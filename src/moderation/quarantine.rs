use std::{
    collections::HashMap,
    hash::{BuildHasher as _, RandomState},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio::sync::OwnedMutexGuard;
use twilight_http::request::AuditLogReason as _;
use twilight_model::{
    application::interaction::Interaction,
    channel::message::{AllowedMentions, Component, MessageFlags, component::ButtonStyle},
    guild::Permissions,
    id::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
    },
    user::User,
};

use crate::{
    app::{self, App},
    db::{
        self,
        quarantine::{Reason, Record, State},
    },
    discord::{
        errors, reply,
        ui::{self, Card, color},
    },
    guild::{ChannelKind, RoleKind},
};

pub const CASE_PREFIX: &str = "case:";
const REMINDER_COOLDOWN: Duration = Duration::from_secs(5 * 60);

type MemberKey = (Id<GuildMarker>, Id<UserMarker>);

#[derive(Default)]
pub struct MemberLocks {
    locks: Mutex<HashMap<MemberKey, Arc<tokio::sync::Mutex<()>>>>,
    reminders: Mutex<HashMap<MemberKey, Instant>>,
}

impl MemberLocks {
    pub async fn lock(&self, guild: Id<GuildMarker>, user: Id<UserMarker>) -> OwnedMutexGuard<()> {
        let lock = {
            let mut map = self
                .locks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Arc::clone(map.entry((guild, user)).or_default())
        };
        lock.lock_owned().await
    }

    pub fn should_remind(&self, guild: Id<GuildMarker>, user: Id<UserMarker>) -> bool {
        let mut map = self
            .reminders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        match map.get(&(guild, user)) {
            Some(at) if now.duration_since(*at) < REMINDER_COOLDOWN => false,
            _ => {
                map.insert((guild, user), now);
                true
            }
        }
    }

    pub fn sweep(&self) {
        self.locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, lock| Arc::strong_count(lock) > 1);
        let now = Instant::now();
        self.reminders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, at| now.duration_since(*at) < REMINDER_COOLDOWN);
    }
}

#[derive(Clone, Debug)]
pub struct Evidence {
    pub summary: String,
    pub excerpt: String,
    pub channel: Option<Id<ChannelMarker>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum QuarantineOutcome {
    Quarantined,
    AlreadyQuarantined,
    /// An open automatic case became a staff case, so only staff can release it.
    Upgraded,
    /// The moderator does not outrank the roles the open case saved.
    Refused(&'static str),
    NotConfigured,
}

pub enum Releaser {
    Token(String),
    /// `roles` are the moderator's. `card` must belong to the open case: a stale card can still
    /// have live buttons.
    Moderator {
        by: Id<UserMarker>,
        roles: Vec<Id<RoleMarker>>,
        card: Option<Id<MessageMarker>>,
    },
    /// Startup found the quarantine role gone; finish the release.
    Reconcile,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReleaseOutcome {
    Released,
    /// A moderator closed the case of a member who had left; no roles were given back.
    ClosedGone,
    NotQuarantined,
    WrongToken,
    StaffOnly,
    StaleCard,
    Failed(String),
}

pub fn closed_gone(user: Id<UserMarker>) -> Vec<Component> {
    ui::success(
        "ปิดเคสแล้ว",
        &format!("<@{user}> ออกจากเซิร์ฟเวอร์ไปแล้ว\n-# ไม่มี role ให้คืน"),
    )
}

fn new_token() -> String {
    format!(
        "{:06}",
        RandomState::new().hash_one(Instant::now()) % 1_000_000
    )
}

/// Staff cases, and automatic ones whose saved roles carry staff powers: no `/verify`, rank-checked
/// release, and a hand removal doesn't release them. Unknown roles count as staff.
fn staff_only(app: &App, record: &Record) -> bool {
    let guild = record.guild_id;
    record.reason == Reason::Manual
        || record.kicked
        || app.cache.guild_roles(guild).is_none()
        || record
            .roles
            .iter()
            .any(|role| crate::features::roles::existing_role_grants_staff(app, guild, *role))
}

pub fn verify_token<'a>(app: &App, record: &'a Record) -> Option<&'a str> {
    (!staff_only(app, record)).then_some(record.token.as_str())
}

pub fn blocked_text(app: &App, record: &Record) -> &'static str {
    if verify_token(app, record).is_some() {
        "คุณถูกกักกันอยู่ ใช้ `/verify` ก่อน"
    } else {
        "คุณถูกกักกันอยู่ รอทีมงานปล่อย"
    }
}

pub fn reason_label(reason: Reason) -> &'static str {
    match reason {
        Reason::Spam => "สแปม",
        Reason::Scam => "ภาพหลอกลวง",
        Reason::Manual => "ทีมงานกักกัน",
    }
}

enum Lookup {
    Roles(Vec<Id<RoleMarker>>),
    Gone,
    Unknown,
}

/// From Discord, not the cache, which can miss a change just made under the same lock. A failed
/// request doesn't mean the member left.
async fn lookup_roles(app: &App, guild: Id<GuildMarker>, user: Id<UserMarker>) -> Lookup {
    match app.http.guild_member(guild, user).await {
        Ok(response) => match crate::discord::read(response).await {
            Ok(member) => Lookup::Roles(member.roles),
            Err(_) => Lookup::Unknown,
        },
        Err(error) if errors::is(&error, errors::UNKNOWN_MEMBER) => Lookup::Gone,
        Err(_) => Lookup::Unknown,
    }
}

async fn fetch_member(
    app: &App,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
) -> anyhow::Result<twilight_model::guild::Member> {
    crate::discord::read(
        app.http
            .guild_member(guild, user)
            .await?,
    )
    .await
}

fn rejoined_since(
    joined_at: Option<twilight_model::util::Timestamp>,
    record: &Record,
) -> Option<bool> {
    joined_at.map(|at| at.as_secs() > record.created_at)
}

/// The Quarantine role plus roles the bot can't manage; the rest go to `saved`.
fn plan_swap(
    app: &App,
    guild: Id<GuildMarker>,
    q_role: Id<RoleMarker>,
    current: &[Id<RoleMarker>],
    saved: &mut Vec<Id<RoleMarker>>,
) -> Vec<Id<RoleMarker>> {
    let mut target = vec![q_role];
    for &role in current {
        if role == q_role {
            continue;
        }
        if app.can_manage_role(guild, role) {
            if !saved.contains(&role) {
                saved.push(role);
            }
        } else {
            target.push(role);
        }
    }
    target
}

enum SwapError {
    /// A 4xx on the first attempt: nothing changed.
    Refused(twilight_http::Error),
    /// A timeout or 5xx that a retry did not clear up; it may or may not have been applied.
    Unknown(twilight_http::Error),
}

async fn swap_roles(
    app: &App,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    target: &[Id<RoleMarker>],
    audit: &str,
) -> Result<(), SwapError> {
    let Err(error) = app
        .http
        .update_guild_member(guild, user)
        .roles(target)
        .reason(audit)
        .await
    else {
        return Ok(());
    };
    if errors::refused(&error) {
        return Err(SwapError::Refused(error));
    }
    // A read right after can lag the change; resending the same roles is harmless.
    match app
        .http
        .update_guild_member(guild, user)
        .roles(target)
        .reason(audit)
        .await
    {
        Ok(_) => {
            tracing::warn!(guild = guild.get(), user = user.get(), %error, "role swap response was lost; the retry applied it");
            Ok(())
        }
        Err(_) => Err(SwapError::Unknown(error)),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Reconcile {
    Keep,
    MarkActive,
    Reapply,
    FinishSwap,
    Release,
}

fn reconcile_action(
    record: &Record,
    staff_only: bool,
    holds_role: bool,
    rejoined: Option<bool>,
) -> Reconcile {
    match (record.state, holds_role) {
        (State::Active, true) => Reconcile::Keep,
        // Staff may have given the role by hand; finishing is safe either way.
        (State::Applying, _) => Reconcile::FinishSwap,
        // The role is on: a crashed release never got that far, or a replacement role was given.
        (State::Releasing | State::Reapply, true) => Reconcile::MarkActive,
        (State::Reapply, false) => Reconcile::Reapply,
        // Staff or /verify started this release; finish it.
        (State::Releasing, false) => Reconcile::Release,
        (State::Active, false) => match rejoined {
            None => Reconcile::Keep,
            // A join the bot never handled brought them back without the role.
            Some(true) => Reconcile::Reapply,
            // Staff-only cases end only through rank-checked releases, not a hand removal.
            Some(false) if staff_only => Reconcile::Reapply,
            Some(false) => Reconcile::Release,
        },
    }
}

/// Rechecks the current record under the member lock before startup releases it.
fn reconcile_may_release(record: &Record, staff_only: bool, role_exists: bool) -> bool {
    role_exists
        && match record.state {
            State::Releasing => true,
            State::Active => !staff_only,
            State::Applying | State::Reapply => false,
        }
}

/// Unknown (guild not cached) counts as present.
pub(crate) fn role_exists(app: &App, guild: Id<GuildMarker>, role: Id<RoleMarker>) -> bool {
    app.cache
        .guild_roles(guild)
        .is_none_or(|roles| roles.contains(&role))
}

async fn finish_swap(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    q_role: Id<RoleMarker>,
) {
    let _guard = app
        .moderation
        .locks
        .lock(guild, user)
        .await;
    let Ok(Some(mut record)) = app
        .db
        .call(move |c| db::quarantine::get(c, guild, user))
        .await
    else {
        return;
    };
    if record.state != State::Applying {
        return;
    }
    if crate::features::roles::grants_staff(app, guild, q_role) {
        tracing::error!(
            guild = guild.get(),
            role = q_role.get(),
            "quarantine role has staff permissions; not applying it"
        );
        return;
    }
    let Lookup::Roles(current) = lookup_roles(app, guild, user).await else {
        return;
    };
    let target = plan_swap(app, guild, q_role, &current, &mut record.roles);
    let roles = record.roles.clone();
    if app
        .db
        .call(move |c| db::quarantine::set_roles(c, guild, user, &roles))
        .await
        .is_err()
    {
        return;
    }
    let audit = format!("quarantine: {}", reason_label(record.reason));
    match swap_roles(app, guild, user, &target, &audit).await {
        Ok(()) => {
            let _ = app
                .db
                .call(move |c| db::quarantine::set_state(c, guild, user, State::Active))
                .await;
            tracing::info!(
                guild = guild.get(),
                user = user.get(),
                "unconfirmed quarantine swap finished"
            );
            remind(app, guild, user, &record).await;
        }
        Err(SwapError::Refused(error) | SwapError::Unknown(error)) => {
            tracing::error!(guild = guild.get(), user = user.get(), %error, "quarantine swap still not confirmed");
        }
    }
}

/// May be stale by the time the member lock is held.
#[derive(Clone, Copy)]
struct Listed<'a> {
    states: &'a [State],
    created_at: i64,
}

impl Listed<'static> {
    fn of(record: &Record) -> Self {
        let states: &'static [State] = match record.state {
            State::Applying => &[State::Applying],
            State::Active => &[State::Active],
            State::Releasing => &[State::Releasing],
            State::Reapply => &[State::Reapply],
        };
        Self { states, created_at: record.created_at }
    }
}

impl Listed<'_> {
    fn matches(&self, record: &Record) -> bool {
        self.states.contains(&record.state) && record.created_at == self.created_at
    }
}

async fn mark_active(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    listed: Listed<'static>,
) {
    let _guard = app
        .moderation
        .locks
        .lock(guild, user)
        .await;
    let _ = app
        .db
        .call(move |c| {
            if db::quarantine::get(c, guild, user)?.is_some_and(|r| listed.matches(&r)) {
                db::quarantine::set_state(c, guild, user, State::Active)?;
            }
            Ok(())
        })
        .await;
}

pub async fn reapply_open_cases(app: &Arc<App>, guild: Id<GuildMarker>, q_role: Id<RoleMarker>) {
    let records = match app.db.call(db::quarantine::list).await {
        Ok(records) => records,
        Err(error) => {
            tracing::warn!(%error, "failed to list quarantines");
            return;
        }
    };
    for record in records
        .into_iter()
        .filter(|r| r.guild_id == guild)
    {
        // Decided under the member lock: a release in progress may fail and reopen the case.
        let listed =
            Listed { states: &[State::Active, State::Reapply], created_at: record.created_at };
        match record.state {
            State::Applying => finish_swap(app, guild, record.user_id, q_role).await,
            State::Active | State::Reapply | State::Releasing => {
                reapply(app, guild, record.user_id, q_role, false, listed).await;
            }
        }
    }
}

async fn reapply(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    q_role: Id<RoleMarker>,
    notify: bool,
    listed: Listed<'_>,
) {
    let _guard = app
        .moderation
        .locks
        .lock(guild, user)
        .await;
    // A release in progress (or done) since the case was listed must not get the role back.
    let Ok(Some(record)) = app
        .db
        .call(move |c| db::quarantine::get(c, guild, user))
        .await
    else {
        return;
    };
    if !listed.matches(&record) {
        return;
    }
    if crate::features::roles::grants_staff(app, guild, q_role) {
        tracing::error!(
            guild = guild.get(),
            role = q_role.get(),
            "quarantine role has staff permissions; not re-applying it"
        );
        return;
    }
    match app
        .http
        .add_guild_member_role(guild, user, q_role)
        .reason("re-applying quarantine")
        .await
    {
        Ok(_) => {
            tracing::info!(
                guild = guild.get(),
                user = user.get(),
                "quarantine role re-applied"
            );
            if record.state == State::Reapply
                && let Err(error) = app
                    .db
                    .call(move |c| db::quarantine::set_state(c, guild, user, State::Active))
                    .await
            {
                tracing::warn!(%error, "failed to clear the reapply mark");
            }
            if notify {
                remind(app, guild, user, &record).await;
            }
        }
        Err(error) => {
            tracing::error!(guild = guild.get(), user = user.get(), %error, "failed to re-apply quarantine")
        }
    }
}

/// The bot strips roles below its own, so without this a moderator could demote an admin.
pub fn may_moderate(
    app: &App,
    guild: Id<GuildMarker>,
    moderator: (Id<UserMarker>, &[Id<RoleMarker>]),
    target: (Id<UserMarker>, &[Id<RoleMarker>]),
) -> Result<(), &'static str> {
    let owner = app
        .cache
        .guild(guild)
        .map(|g| g.owner_id());
    outranks(owner, moderator, target, |id| {
        app.cache.role(id).map(|r| {
            (
                r.position,
                r.permissions
                    .contains(Permissions::ADMINISTRATOR),
            )
        })
    })
}

fn outranks(
    owner: Option<Id<UserMarker>>,
    (moderator, moderator_roles): (Id<UserMarker>, &[Id<RoleMarker>]),
    (target, target_roles): (Id<UserMarker>, &[Id<RoleMarker>]),
    role: impl Fn(Id<RoleMarker>) -> Option<(i64, bool)>,
) -> Result<(), &'static str> {
    if owner == Some(moderator) {
        return Ok(());
    }
    if owner == Some(target) {
        return Err("เป็นเจ้าของเซิร์ฟเวอร์");
    }
    if target_roles
        .iter()
        .any(|r| role(*r).is_some_and(|(_, admin)| admin))
    {
        return Err("มีสิทธิ์ Administrator");
    }
    let top = |roles: &[Id<RoleMarker>]| {
        roles
            .iter()
            .filter_map(|r| role(*r).map(|(position, _)| position))
            .max()
            .unwrap_or(0)
    };
    if top(target_roles) >= top(moderator_roles) {
        return Err("role สูงเท่าหรือสูงกว่าคุณ");
    }
    Ok(())
}

/// Staff-only cases follow the rank rule, but any Administrator may release them.
fn release_refused(
    app: &App,
    guild: Id<GuildMarker>,
    staff_only: bool,
    moderator: (Id<UserMarker>, &[Id<RoleMarker>]),
    target: (Id<UserMarker>, &[Id<RoleMarker>]),
) -> Option<&'static str> {
    if !staff_only
        || super::holds_any(
            app,
            guild,
            moderator.0,
            moderator.1,
            Permissions::ADMINISTRATOR,
        )
    {
        return None;
    }
    may_moderate(app, guild, moderator, target).err()
}

pub async fn quarantine(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    user: &User,
    reason: Reason,
    evidence: Evidence,
    by: Option<(Id<UserMarker>, &[Id<RoleMarker>])>,
) -> anyhow::Result<QuarantineOutcome> {
    let by_id = by.map(|(id, _)| id);
    // Held until the record exists, so the panel can't switch the role. Always taken before the
    // member lock.
    let config_guard = app
        .moderation
        .quarantine_config
        .read()
        .await;
    let config = app.guild(guild);
    let (Some(q_role), Some(q_channel)) = (
        config.role_id(RoleKind::Quarantine),
        config.channel(ChannelKind::Quarantine),
    ) else {
        return Ok(QuarantineOutcome::NotConfigured);
    };
    // Every spammer gets this role; one with staff powers would hand them out.
    if crate::features::roles::grants_staff(app, guild, q_role) {
        tracing::error!(
            guild = guild.get(),
            role = q_role.get(),
            "quarantine role has staff permissions; not using it"
        );
        return Ok(QuarantineOutcome::NotConfigured);
    }
    let _guard = app
        .moderation
        .locks
        .lock(guild, user.id)
        .await;
    let user_id = user.id;
    let existing = app
        .db
        .call(move |c| db::quarantine::get(c, guild, user_id))
        .await?;
    let newly_created = existing.is_none();
    let upgrade = by.is_some() && reason == Reason::Manual;
    // An open case already stripped the roles that matter, so also outrank the saved ones.
    let outranked = |record: &Record| {
        by.and_then(|(id, roles)| {
            may_moderate(app, guild, (id, roles), (user_id, &record.roles)).err()
        })
    };
    if let Some(record) = &existing
        && upgrade
        && record.reason != Reason::Manual
        && let Some(reason) = outranked(record)
    {
        return Ok(QuarantineOutcome::Refused(reason));
    }
    let mut record = match existing {
        // An unconfirmed swap or restore: swap from the roles the member has now.
        Some(mut record) if matches!(record.state, State::Applying | State::Releasing) => {
            if record.state == State::Releasing {
                app.db
                    .call(move |c| db::quarantine::set_state(c, guild, user_id, State::Applying))
                    .await?;
                record.state = State::Applying;
            }
            if upgrade && record.reason != Reason::Manual {
                app.db
                    .call(move |c| db::quarantine::set_reason(c, guild, user_id, Reason::Manual))
                    .await?;
                record.reason = Reason::Manual;
            }
            record
        }
        Some(record) if upgrade && record.reason != Reason::Manual => {
            app.db
                .call(move |c| db::quarantine::set_reason(c, guild, user_id, Reason::Manual))
                .await?;
            tracing::info!(
                guild = guild.get(),
                user = user_id.get(),
                by = by_id.map(|id| id.get()),
                "automatic case upgraded to a staff case"
            );
            if let (Some((channel, card)), Some(id)) = (record.case_message, by_id) {
                let note = ui::notice(
                    color::MUTED,
                    "เปลี่ยนเป็นเคสทีมงาน",
                    &format!("โดย <@{id}>"),
                );
                let app2 = Arc::clone(app);
                app.spawn_outbound("case_upgraded", async move {
                    if let Err(error) = app2
                        .http
                        .create_message(channel)
                        .flags(MessageFlags::IS_COMPONENTS_V2)
                        .reply(card)
                        .fail_if_not_exists(false)
                        .components(&note)
                        .await
                    {
                        tracing::warn!(%error, "failed to note the case upgrade");
                    }
                });
            }
            return Ok(QuarantineOutcome::Upgraded);
        }
        Some(_) => return Ok(QuarantineOutcome::AlreadyQuarantined),
        None => Record {
            guild_id: guild,
            user_id,
            token: new_token(),
            roles: Vec::new(),
            state: State::Applying,
            reason,
            evidence: format!("{}\n{}", evidence.summary, evidence.excerpt),
            case_message: None,
            created_at: db::now(),
            kicked: false,
        },
    };
    let current = match lookup_roles(app, guild, user_id).await {
        Lookup::Roles(current) => current,
        Lookup::Gone => anyhow::bail!("member {user_id} not found in guild {guild}"),
        Lookup::Unknown => anyhow::bail!("could not read the roles of {user_id} in guild {guild}"),
    };
    let target = plan_swap(app, guild, q_role, &current, &mut record.roles);
    if newly_created {
        let stored = record.clone();
        if !app
            .db
            .call(move |c| db::quarantine::insert_new(c, &stored))
            .await?
        {
            return Ok(QuarantineOutcome::AlreadyQuarantined);
        }
    } else {
        let roles = record.roles.clone();
        app.db
            .call(move |c| db::quarantine::set_roles(c, guild, user_id, &roles))
            .await?;
    }
    drop(config_guard);

    let audit = format!("quarantine: {}", reason_label(record.reason));
    match swap_roles(app, guild, user_id, &target, &audit).await {
        Ok(()) => {
            app.db
                .call(move |c| db::quarantine::set_state(c, guild, user_id, State::Active))
                .await?;
            record.state = State::Active;
        }
        Err(SwapError::Refused(error)) if newly_created => {
            tracing::error!(guild = guild.get(), user = user_id.get(), %error, "role swap refused; dropping the new case");
            app.db
                .call(move |c| db::quarantine::delete(c, guild, user_id))
                .await?;
            return Err(error.into());
        }
        Err(SwapError::Refused(error) | SwapError::Unknown(error)) => {
            tracing::error!(guild = guild.get(), user = user_id.get(), %error, "role swap not confirmed; the case stays open and the swap is retried");
            return Err(error.into());
        }
    }
    let reason = record.reason;
    metrics::counter!("quarantines_total", "reason" => reason.as_str()).increment(1);
    tracing::info!(
        guild = guild.get(),
        user = user.id.get(),
        reason = reason.as_str(),
        by = by_id.map(|id| id.get()),
        "member quarantined"
    );

    let notice = Card::new(color::DANGER)
        .title("คุณถูกกักกันชั่วคราว")
        .text(format!(
            "<@{}> เหตุผล: **{}**{}\n{}",
            user.id,
            reason_label(reason),
            // Scam evidence carries the detector's score and terms, which are for moderators.
            if reason == Reason::Scam || evidence.summary.is_empty() {
                String::new()
            } else {
                format!(" — {}", evidence.summary)
            },
            if verify_token(app, &record).is_some() {
                "พิมพ์ `/verify` ในช่องนี้เพื่อกลับมาใช้งาน"
            } else {
                "รอทีมงานปล่อย"
            }
        ));
    let notice = match verify_token(app, &record) {
        Some(token) => notice.small(format!(
            "Token `{token}` · ถ้าเข้าใจผิด ทีมงานปล่อยให้ได้"
        )),
        None => notice,
    }
    .build();
    send_with_ping(app, q_channel, user.id, &[notice]).await;

    if let Some(log) = config.channel(ChannelKind::ModLog) {
        let card = case_card(&record, user, &evidence, by_id, CaseState::Open);
        match app
            .http
            .create_message(log)
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(&card)
            .await
        {
            // Without the saved id the card's buttons answer as an old case.
            Ok(response) => match crate::discord::read(response).await {
                Ok(message) => {
                    let id = message.id;
                    if let Err(error) = app
                        .db
                        .call(move |c| db::quarantine::set_case_message(c, guild, user_id, log, id))
                        .await
                    {
                        tracing::warn!(%error, "failed to save moderation case card");
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to read moderation case card"),
            },
            Err(error) => tracing::warn!(%error, "failed to post moderation case"),
        }
    }
    Ok(QuarantineOutcome::Quarantined)
}

async fn send_with_ping(
    app: &App,
    channel: Id<ChannelMarker>,
    user: Id<UserMarker>,
    components: &[Component],
) {
    let mut parts = Vec::with_capacity(components.len() + 1);
    parts.push(Component::TextDisplay(
        twilight_model::channel::message::component::TextDisplay {
            id: None,
            content: format!("<@{user}>"),
        },
    ));
    parts.extend_from_slice(components);
    let allowed = AllowedMentions { users: vec![user], ..Default::default() };
    if let Err(error) = app
        .http
        .create_message(channel)
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(&parts)
        .allowed_mentions(Some(&allowed))
        .await
    {
        tracing::warn!(channel = channel.get(), %error, "failed to post quarantine notice");
    }
}

#[derive(Clone, Copy)]
pub enum CaseState<'a> {
    Open,
    /// Still open (buttons kept), with a note such as a kick.
    Noted(&'a str),
    Closed(&'a str),
}

pub fn case_card(
    record: &Record,
    user: &User,
    evidence: &Evidence,
    by: Option<Id<UserMarker>>,
    state: CaseState<'_>,
) -> Vec<Component> {
    let accent = if matches!(state, CaseState::Closed(_)) { color::MUTED } else { color::DANGER };
    let mut header = format!(
        "### เคสกักกัน · {}\n<@{}> · `{}`\n**เหตุผล** {}",
        ui::escape_markdown(app::display_name(user)),
        user.id,
        user.id,
        reason_label(record.reason),
    );
    if !evidence.summary.is_empty() {
        header.push_str(&format!(" — {}", evidence.summary));
    }
    if let Some(by) = by {
        header.push_str(&format!("\n**โดย** <@{by}>"));
    }
    let mut card = Card::new(accent).section(header, Some(&app::avatar_url(user)));
    if !evidence.excerpt.trim().is_empty() {
        // Escaping can double user text; the card must stay under 4000 characters.
        card = card.text(format!(
            ">>> {}",
            ui::truncate(&ui::escape_markdown(&evidence.excerpt), 2400)
        ));
    }
    let channel = evidence
        .channel
        .map(|c| format!("<#{c}> · "))
        .unwrap_or_default();
    card = card.small(format!(
        "{channel}{} · เก็บ role ไว้ {} รายการ",
        ui::ts(record.created_at * 1000, 'R'),
        record.roles.len()
    ));
    if let CaseState::Noted(text) | CaseState::Closed(text) = state {
        card = card.divider().text(text);
    }
    card = match state {
        CaseState::Closed(_) => card,
        CaseState::Open | CaseState::Noted(_) => card.buttons([
            ui::button_with(
                ButtonStyle::Success,
                format!("{CASE_PREFIX}release:{}", user.id),
                "ปล่อย",
                false,
            ),
            ui::button_with(
                ButtonStyle::Danger,
                format!("{CASE_PREFIX}kick:{}", user.id),
                "Kick",
                false,
            ),
        ]),
    };
    vec![card.build()]
}

pub fn reminder_card(
    app: &App,
    guild: Id<GuildMarker>,
    channel: Id<ChannelMarker>,
    record: &Record,
) -> Vec<Component> {
    let name = ui::escape_markdown(&app.guild_name(guild));
    let card = Card::new(color::WARNING).title("ยังถูกกักกันอยู่");
    let card = match verify_token(app, record) {
        Some(token) => card
            .text(format!(
                "พิมพ์ `/verify` ใน <#{channel}> เพื่อกลับมาใช้งาน"
            ))
            .small(format!("{name} · Token `{token}`")),
        None => card
            .text(format!("รอทีมงานปล่อย ติดต่อได้ที่ <#{channel}>"))
            .small(name),
    };
    vec![card.build()]
}

pub async fn remind(app: &Arc<App>, guild: Id<GuildMarker>, user: Id<UserMarker>, record: &Record) {
    let Some(channel) = app
        .guild(guild)
        .channel(ChannelKind::Quarantine)
    else {
        return;
    };
    if !app
        .moderation
        .locks
        .should_remind(guild, user)
    {
        return;
    }
    send_with_ping(
        app,
        channel,
        user,
        &reminder_card(app, guild, channel, record),
    )
    .await;
}

pub fn delete_messages(app: &Arc<App>, messages: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>) {
    let app2 = Arc::clone(app);
    app.spawn(async move {
        for (channel, message) in messages {
            if let Err(error) = app2
                .http
                .delete_message(channel, message)
                .await
                && !errors::is(&error, errors::UNKNOWN_MESSAGE)
            {
                tracing::warn!(channel = channel.get(), %error, "failed to delete flagged message");
            }
        }
    });
}

const STILL_QUARANTINED: &str = "still quarantined";
const NOT_FOR_RECONCILE: &str = "not released by startup";

pub async fn release(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    releaser: Releaser,
) -> anyhow::Result<ReleaseOutcome> {
    let by = match &releaser {
        Releaser::Moderator { by, .. } => Some(by.get()),
        Releaser::Token(_) => Some(user.get()),
        Releaser::Reconcile => None,
    };
    let outcome = release_locked(app, guild, user, releaser).await;
    // The reason reaches people only as the reply, which is lost if the interaction expired.
    if let (Ok(ReleaseOutcome::Failed(reason)), Some(by)) = (&outcome, by) {
        tracing::warn!(
            guild = guild.get(),
            user = user.get(),
            by,
            reason,
            "release refused"
        );
    }
    outcome
}

async fn release_locked(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    releaser: Releaser,
) -> anyhow::Result<ReleaseOutcome> {
    let _guard = app
        .moderation
        .locks
        .lock(guild, user)
        .await;
    let Some(record) = app
        .db
        .call(move |c| db::quarantine::get(c, guild, user))
        .await?
    else {
        return Ok(ReleaseOutcome::NotQuarantined);
    };
    if let Releaser::Token(token) = &releaser {
        if verify_token(app, &record).is_none() {
            return Ok(ReleaseOutcome::StaffOnly);
        }
        if *token != record.token {
            return Ok(ReleaseOutcome::WrongToken);
        }
    }
    if let Releaser::Moderator { card: Some(card), .. } = &releaser
        && !is_case_card(&record, *card)
    {
        return Ok(ReleaseOutcome::StaleCard);
    }
    if matches!(releaser, Releaser::Reconcile)
        && !app
            .guild(guild)
            .role_id(RoleKind::Quarantine)
            .is_some_and(|role| {
                reconcile_may_release(
                    &record,
                    staff_only(app, &record),
                    role_exists(app, guild, role),
                )
            })
    {
        return Ok(ReleaseOutcome::Failed(NOT_FOR_RECONCILE.into()));
    }
    if let Releaser::Moderator { by, .. } = &releaser
        && *by == user
    {
        return Ok(ReleaseOutcome::Failed("ปล่อยตัวเองไม่ได้".into()));
    }
    let staff_only = staff_only(app, &record);
    let outranked = |current: &[Id<RoleMarker>]| {
        let Releaser::Moderator { by, roles, .. } = &releaser else { return None };
        let ranked: Vec<_> = current
            .iter()
            .chain(&record.roles)
            .copied()
            .collect();
        release_refused(
            app,
            guild,
            staff_only,
            (*by, roles),
            (user, &ranked),
        )
        .map(|reason| ReleaseOutcome::Failed(format!("ปล่อยไม่ได้: {reason}")))
    };

    // The saved roles alone may rule it out, before asking Discord.
    if let Some(refused) = outranked(&[]) {
        return Ok(refused);
    }
    let q_role = app
        .guild(guild)
        .role_id(RoleKind::Quarantine);
    let current = match lookup_roles(app, guild, user).await {
        Lookup::Roles(current) => current,
        Lookup::Gone if let Releaser::Moderator { by, .. } = &releaser => {
            app.db
                .call(move |c| db::quarantine::delete(c, guild, user))
                .await?;
            app.moderation.spam.forget(guild, user);
            // The saved roles are gone for good, so the log keeps who closed it.
            tracing::info!(
                guild = guild.get(),
                user = user.get(),
                by = by.get(),
                roles = ?record.roles,
                "case closed for a member who left"
            );
            let text = format!(
                "ปิดเคสโดย <@{by}> · {} · สมาชิกออกจากเซิร์ฟเวอร์ไปแล้ว",
                ui::ts(db::now() * 1000, 'R')
            );
            finish_case(app, &record, &text).await;
            return Ok(ReleaseOutcome::ClosedGone);
        }
        Lookup::Gone => return Ok(ReleaseOutcome::Failed("ไม่พบสมาชิก".into())),
        Lookup::Unknown => return Ok(ReleaseOutcome::Failed("Discord ขัดข้อง".into())),
    };
    if matches!(releaser, Releaser::Reconcile) && q_role.is_some_and(|role| current.contains(&role))
    {
        return Ok(ReleaseOutcome::Failed(STILL_QUARANTINED.into()));
    }
    if let Some(refused) = outranked(&current) {
        return Ok(refused);
    }

    let mut target: Vec<Id<RoleMarker>> = current
        .into_iter()
        .filter(|r| Some(*r) != q_role)
        .collect();
    let (mut skipped, mut blocked) = (0usize, 0usize);
    for role in &record.roles {
        if Some(*role) == q_role || target.contains(role) {
            continue;
        }
        if !role_exists(app, guild, *role) {
            skipped += 1;
        } else if app.can_manage_role(guild, *role) {
            target.push(*role);
        } else {
            blocked += 1;
        }
    }
    // A saved role now above the bot would be lost; keep the case open until that's fixed.
    if blocked > 0 {
        return Ok(ReleaseOutcome::Failed("role อยู่สูงกว่าบอท".into()));
    }
    app.db
        .call(move |c| db::quarantine::set_state(c, guild, user, State::Releasing))
        .await?;

    if let Err(error) = app
        .http
        .update_guild_member(guild, user)
        .roles(&target)
        .reason("quarantine released")
        .await
    {
        let released = !errors::refused(&error)
            && match lookup_roles(app, guild, user).await {
                Lookup::Roles(now) => {
                    target.iter().all(|r| now.contains(r))
                        && q_role.is_none_or(|role| !now.contains(&role))
                }
                Lookup::Gone | Lookup::Unknown => false,
            };
        if released {
            tracing::warn!(guild = guild.get(), user = user.get(), %error, "role restore response was lost but Discord applied it");
        } else {
            // A refusal changed nothing; otherwise startup decides from the member's real roles.
            if errors::refused(&error) {
                let previous = record.state;
                app.db
                    .call(move |c| db::quarantine::set_state(c, guild, user, previous))
                    .await?;
            }
            let reason = if errors::is_forbidden(&error) {
                "บอทไม่มีสิทธิ์คืน role บางรายการ"
            } else {
                "Discord ขัดข้อง"
            };
            tracing::error!(guild = guild.get(), user = user.get(), %error, "failed to restore roles");
            return Ok(ReleaseOutcome::Failed(reason.into()));
        }
    }

    app.db
        .call(move |c| db::quarantine::delete(c, guild, user))
        .await?;
    app.moderation.spam.forget(guild, user);
    let mut text = match releaser {
        Releaser::Token(_) => format!("ยืนยันตัวตนแล้ว · {}", ui::ts(db::now() * 1000, 'R')),
        Releaser::Reconcile if record.state == State::Releasing => {
            "ปล่อยแล้ว (ทำต่อจากที่ค้างไว้)".to_owned()
        }
        Releaser::Reconcile => "ปล่อยแล้ว (มีคนถอด role กักกันเอง)".to_owned(),
        Releaser::Moderator { by, .. } => format!(
            "ปล่อยโดย <@{by}> · {}",
            ui::ts(db::now() * 1000, 'R')
        ),
    };
    if skipped > 0 {
        text.push_str(&format!("\nคืนไม่ได้ {skipped} role (ถูกลบไปแล้ว)"));
    }
    tracing::info!(
        guild = guild.get(),
        user = user.get(),
        by = match releaser {
            Releaser::Token(_) => "verify",
            Releaser::Moderator { .. } => "moderator",
            Releaser::Reconcile => "reconcile",
        },
        "quarantine released"
    );
    finish_case(app, &record, &text).await;
    Ok(ReleaseOutcome::Released)
}

async fn finish_case(app: &App, record: &Record, text: &str) {
    update_case(app, record, CaseState::Closed(text)).await;
}

async fn update_case(app: &App, record: &Record, state: CaseState<'_>) {
    let Some((channel, message)) = record.case_message else {
        return;
    };
    let user = match app.http.user(record.user_id).await {
        Ok(response) => match crate::discord::read(response).await {
            Ok(user) => user,
            Err(_) => return,
        },
        Err(_) => return,
    };
    let (summary, excerpt) = record
        .evidence
        .split_once('\n')
        .unwrap_or((record.evidence.as_str(), ""));
    let evidence =
        Evidence { summary: summary.to_owned(), excerpt: excerpt.to_owned(), channel: None };
    let card = case_card(record, &user, &evidence, None, state);
    if let Err(error) = app
        .http
        .update_message(channel, message)
        .components(Some(&card))
        .await
    {
        tracing::debug!(%error, "failed to update case card");
    }
}

pub async fn reconcile_guild(app: &Arc<App>, guild: Id<GuildMarker>) {
    // Held for the whole pass, so the Quarantine role can't change midway.
    let _config = app
        .moderation
        .quarantine_config
        .read()
        .await;
    let Some(q_role) = app
        .guild(guild)
        .role_id(RoleKind::Quarantine)
    else {
        return;
    };
    let records: Vec<Record> = match app.db.call(db::quarantine::list).await {
        Ok(records) => records
            .into_iter()
            .filter(|r| r.guild_id == guild)
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "failed to list quarantines");
            return;
        }
    };
    // A role deleted while offline is still bound; members lacking it must not read as released.
    if !role_exists(app, guild, q_role) {
        tracing::warn!(
            guild = guild.get(),
            role = q_role.get(),
            cases = records.len(),
            "quarantine role no longer exists; spam and scam detection are off and cases wait for a new one"
        );
        if let Err(error) = app
            .db
            .call(move |c| db::quarantine::mark_active_for_reapply(c, guild))
            .await
        {
            tracing::warn!(%error, "failed to mark cases for reapply");
            return;
        }
        if let Err(error) = app
            .guilds
            .clear_role(guild, q_role)
            .await
        {
            tracing::warn!(%error, "failed to clear the deleted quarantine role");
        }
        return;
    }
    for record in records {
        let user = record.user_id;
        // The cache doesn't keep members. A failed lookup keeps the record, so a rejoin is
        // quarantined again.
        let Ok(member) = fetch_member(app, guild, user).await else {
            continue;
        };
        let holds_role = member.roles.contains(&q_role);
        let joined = rejoined_since(member.joined_at, &record);
        match reconcile_action(
            &record,
            staff_only(app, &record),
            holds_role,
            joined,
        ) {
            Reconcile::Keep => {}
            Reconcile::MarkActive => {
                mark_active(app, guild, user, Listed::of(&record)).await;
            }
            Reconcile::Reapply => {
                let listed = Listed::of(&record);
                reapply(
                    app,
                    guild,
                    user,
                    q_role,
                    record.state != State::Reapply,
                    listed,
                )
                .await;
            }
            Reconcile::FinishSwap => finish_swap(app, guild, user, q_role).await,
            Reconcile::Release => {
                tracing::info!(
                    guild = guild.get(),
                    user = user.get(),
                    "quarantine role is gone; restoring the saved roles"
                );
                match release(app, guild, user, Releaser::Reconcile).await {
                    Ok(ReleaseOutcome::Failed(reason))
                        if reason != STILL_QUARANTINED && reason != NOT_FOR_RECONCILE =>
                    {
                        tracing::warn!(
                            guild = guild.get(),
                            user = user.get(),
                            reason,
                            "reconcile release did not finish"
                        );
                    }
                    Ok(_) => {}
                    Err(error) => tracing::warn!(%error, "reconcile release failed"),
                }
            }
        }
    }
}

pub fn member_permissions(interaction: &Interaction) -> Permissions {
    interaction
        .member
        .as_ref()
        .and_then(|m| m.permissions)
        .unwrap_or_else(Permissions::empty)
}

pub fn member_roles(interaction: &Interaction) -> Vec<Id<RoleMarker>> {
    interaction
        .member
        .as_ref()
        .map(|m| m.roles.clone())
        .unwrap_or_default()
}

fn is_case_card(record: &Record, card: Id<MessageMarker>) -> bool {
    record
        .case_message
        .is_some_and(|(_, message)| message == card)
}

fn stale_card() -> Vec<Component> {
    ui::notice(
        color::MUTED,
        "การ์ดนี้เป็นเคสเก่า",
        "ใช้การ์ดเคสล่าสุด หรือให้แอดมินใช้ `/mod release`",
    )
}

pub async fn on_case_button(
    app: &Arc<App>,
    interaction: &Interaction,
    custom_id: &str,
) -> anyhow::Result<()> {
    let (Some(guild), Some(moderator)) = (interaction.guild_id, interaction.author_id()) else {
        return Ok(());
    };
    let Some((action, user)) = custom_id
        .strip_prefix(CASE_PREFIX)
        .and_then(|rest| rest.split_once(':'))
    else {
        return Ok(());
    };
    let Some(user) = user
        .parse::<u64>()
        .ok()
        .and_then(Id::<UserMarker>::new_checked)
    else {
        return Ok(());
    };
    let perms = member_permissions(interaction);
    let needed =
        if action == "kick" { Permissions::KICK_MEMBERS } else { Permissions::MODERATE_MEMBERS };
    if !perms.intersects(needed | Permissions::ADMINISTRATOR) {
        return reply::respond(
            app,
            interaction,
            ui::error("คุณไม่มีสิทธิ์จัดการเคสนี้"),
            true,
        )
        .await;
    }
    reply::defer(app, interaction, true).await?;
    let card = interaction
        .message
        .as_ref()
        .map(|m| m.id);

    let message = match action {
        "release" => match release(
            app,
            guild,
            user,
            Releaser::Moderator { by: moderator, roles: member_roles(interaction), card },
        )
        .await?
        {
            ReleaseOutcome::Released => {
                ui::success("ปล่อยแล้ว", &format!("คืน role ให้ <@{user}> แล้ว"))
            }
            ReleaseOutcome::ClosedGone => closed_gone(user),
            ReleaseOutcome::NotQuarantined => {
                ui::notice(color::MUTED, "ไม่มีเคสค้าง", "สมาชิกคนนี้ไม่ได้ถูกกักกันแล้ว")
            }
            ReleaseOutcome::StaleCard => stale_card(),
            ReleaseOutcome::WrongToken | ReleaseOutcome::StaffOnly => ui::error("token ไม่ถูกต้อง"),
            ReleaseOutcome::Failed(reason) => ui::error(&reason),
        },
        "kick" => 'kick: {
            let _guard = app
                .moderation
                .locks
                .lock(guild, user)
                .await;
            let Some(open) = app
                .db
                .call(move |c| db::quarantine::get(c, guild, user))
                .await?
            else {
                break 'kick ui::notice(color::MUTED, "ไม่มีเคสค้าง", "สมาชิกคนนี้ไม่ได้ถูกกักกันแล้ว");
            };
            if !card.is_some_and(|card| is_case_card(&open, card)) {
                break 'kick stale_card();
            }
            let moderator_roles = interaction
                .member
                .as_ref()
                .map_or(&[][..], |m| m.roles.as_slice());
            // Quarantine leaves managed roles and roles above the bot, so rank by both.
            let mut ranked = match lookup_roles(app, guild, user).await {
                Lookup::Roles(now) => now,
                Lookup::Gone => Vec::new(),
                Lookup::Unknown => break 'kick ui::error("Discord ขัดข้อง ลองใหม่อีกครั้ง"),
            };
            ranked.extend_from_slice(&open.roles);
            if let Err(reason) = may_moderate(
                app,
                guild,
                (moderator, moderator_roles),
                (user, &ranked),
            ) {
                break 'kick ui::notice(color::WARNING, "Kick ไม่ได้", reason);
            }
            match app
                .http
                .remove_guild_member(guild, user)
                .reason("kicked from quarantine case")
                .await
            {
                // Already gone counts as kicked.
                Ok(_) => {}
                Err(error) if errors::is(&error, errors::UNKNOWN_MEMBER) => {}
                Err(error) if errors::refused(&error) => {
                    tracing::warn!(%error, "kick failed");
                    break 'kick ui::error("Kick ไม่สำเร็จ (ตรวจสอบสิทธิ์ของบอท)");
                }
                // A timeout or 5xx may come after the kick, so check before leaving the case open.
                Err(error) => match lookup_roles(app, guild, user).await {
                    Lookup::Gone => {
                        tracing::warn!(%error, "kick response was lost but the member is gone");
                    }
                    Lookup::Roles(_) | Lookup::Unknown => {
                        tracing::warn!(%error, "kick failed");
                        break 'kick ui::error("Discord ไม่ตอบ กด Kick อีกครั้ง");
                    }
                },
            }
            // Stays open for staff only, so `/verify` can't undo the kick after a rejoin.
            if let Err(error) = app
                .db
                .call(move |c| db::quarantine::set_kicked(c, guild, user))
                .await
            {
                tracing::warn!(%error, "failed to mark the case kicked");
                break 'kick ui::error("Kick แล้ว แต่บันทึกเคสไม่สำเร็จ ลองกด Kick อีกครั้ง");
            }
            let open = Record { kicked: true, ..open };
            let note = format!(
                "Kick โดย <@{moderator}> · {} · เคสยังเปิดอยู่",
                ui::ts(db::now() * 1000, 'R')
            );
            update_case(app, &open, CaseState::Noted(&note)).await;
            ui::success(
                "Kick แล้ว",
                &format!("<@{user}> ถูก kick แล้ว\n-# กลับเข้ามาจะถูกกักกันอีก"),
            )
        }
        _ => return Ok(()),
    };
    reply::edit(app, interaction, &message).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_card_fits_limits_open_and_resolved() {
        let user: User = serde_json::from_value(serde_json::json!({
            "id": "123456789012345678", "username": "spammer", "discriminator": "0",
            "global_name": "ชื่อยาว".repeat(10), "avatar": null
        }))
        .unwrap();
        let record = Record {
            guild_id: Id::new(1),
            user_id: user.id,
            token: "012345".into(),
            roles: vec![Id::new(5); 30],
            state: State::Active,
            reason: Reason::Scam,
            evidence: String::new(),
            case_message: None,
            created_at: 1,
            kicked: false,
        };
        let mut evidence = Evidence {
            summary: "ภาพมีลักษณะหลอกลวง (risk 1.00: casino, bonus, promo code)".into(),
            excerpt: "x".repeat(1200),
            channel: Some(Id::new(9)),
        };
        crate::discord::ui::validate_message(&case_card(
            &record,
            &user,
            &evidence,
            None,
            CaseState::Open,
        ))
        .unwrap();
        evidence.excerpt = format!(
            "{}\nไฟล์แนบ: {}",
            "_".repeat(2000),
            vec!["*".repeat(80); 5].join(", ")
        );
        evidence.summary = "ทีมงานกักกัน ".repeat(25);
        crate::discord::ui::validate_message(&case_card(
            &record,
            &user,
            &evidence,
            Some(Id::new(3)),
            CaseState::Noted("Kick โดย <@3> · เคสยังเปิดอยู่"),
        ))
        .unwrap();
        crate::discord::ui::validate_message(&case_card(
            &record,
            &user,
            &evidence,
            None,
            CaseState::Closed("ปล่อยแล้ว"),
        ))
        .unwrap();
    }

    #[tokio::test]
    async fn staff_cases_cannot_be_released_with_the_token() {
        let app = crate::app::test_app().await;
        let (guild, user) = (Id::new(77), Id::new(9));
        let mut record = Record {
            guild_id: guild,
            user_id: user,
            token: "123456".into(),
            roles: vec![Id::new(600)],
            state: State::Active,
            reason: Reason::Manual,
            evidence: String::new(),
            case_message: None,
            created_at: 0,
            kicked: false,
        };
        assert_eq!(verify_token(&app, &record), None);
        assert!(blocked_text(&app, &record).contains("ทีมงาน"));
        let stored = record.clone();
        app.db
            .call(move |c| db::quarantine::insert_new(c, &stored))
            .await
            .unwrap();
        let outcome = release(
            &app,
            guild,
            user,
            Releaser::Token("123456".into()),
        )
        .await
        .unwrap();
        assert!(matches!(outcome, ReleaseOutcome::StaffOnly));
        assert!(
            app.db
                .call(move |c| db::quarantine::get(c, guild, user))
                .await
                .unwrap()
                .is_some()
        );
        record.reason = Reason::Spam;
        // Until the guild's roles are cached nothing is known about the saved ones.
        assert_eq!(verify_token(&app, &record), None);
        let cache_role = |id: u64, position: i64, permissions: Permissions| {
            let role = serde_json::from_value(serde_json::json!({
                "id": id.to_string(), "name": "r", "color": 0, "hoist": false,
                "colors": {"primary_color": 0}, "position": position,
                "permissions": permissions.bits().to_string(),
                "managed": false, "mentionable": false, "flags": 0
            }))
            .unwrap();
            app.cache.update(
                &twilight_model::gateway::event::Event::RoleCreate(
                    twilight_model::gateway::payload::incoming::RoleCreate {
                        guild_id: guild,
                        role,
                    },
                ),
            );
        };
        cache_role(77, 0, Permissions::empty());
        assert_eq!(verify_token(&app, &record), Some("123456"));
        assert!(blocked_text(&app, &record).contains("/verify"));
        // A kicked case is staff-only too, whatever its reason.
        let kicked = Record { kicked: true, ..record.clone() };
        assert_eq!(verify_token(&app, &kicked), None);
        // A saved role with staff powers makes even an automatic case staff-release only.
        cache_role(600, 8, Permissions::KICK_MEMBERS);
        assert_eq!(verify_token(&app, &record), None);
        // ...and a moderator ranked below that role cannot release it.
        cache_role(601, 5, Permissions::MODERATE_MEMBERS);
        let stored = record.clone();
        app.db
            .call(move |c| db::quarantine::set_reason(c, guild, user, stored.reason))
            .await
            .unwrap();
        let junior = Releaser::Moderator { by: Id::new(3), roles: vec![Id::new(601)], card: None };
        assert!(matches!(
            release(&app, guild, user, junior).await.unwrap(),
            ReleaseOutcome::Failed(reason) if reason.starts_with("ปล่อยไม่ได้")
        ));
        // Nor turn it into a staff case they could not undo.
        cache_role(700, 1, Permissions::empty());
        cache_role(602, 9, Permissions::MODERATE_MEMBERS);
        app.guilds
            .set_role(
                guild,
                RoleKind::Quarantine,
                crate::guild::RoleBinding { id: Id::new(700), self_assignable: false },
            )
            .await
            .unwrap();
        app.guilds
            .set_channel(guild, ChannelKind::Quarantine, Id::new(701))
            .await
            .unwrap();
        let target: User = serde_json::from_value(serde_json::json!({
            "id": "9", "username": "v", "discriminator": "0", "avatar": null
        }))
        .unwrap();
        let evidence =
            || Evidence { summary: String::new(), excerpt: String::new(), channel: None };
        let junior: &[Id<RoleMarker>] = &[Id::new(601)];
        assert!(matches!(
            quarantine(
                &app,
                guild,
                &target,
                Reason::Manual,
                evidence(),
                Some((Id::new(3), junior))
            )
            .await
            .unwrap(),
            QuarantineOutcome::Refused(_)
        ));
        let reason = |app: Arc<App>| async move {
            app.db
                .call(move |c| db::quarantine::get(c, guild, user))
                .await
                .unwrap()
                .unwrap()
                .reason
        };
        assert_eq!(reason(Arc::clone(&app)).await, Reason::Spam);
        let senior: &[Id<RoleMarker>] = &[Id::new(602)];
        assert!(matches!(
            quarantine(
                &app,
                guild,
                &target,
                Reason::Manual,
                evidence(),
                Some((Id::new(4), senior))
            )
            .await
            .unwrap(),
            QuarantineOutcome::Upgraded
        ));
        assert_eq!(reason(Arc::clone(&app)).await, Reason::Manual);
        // Any Administrator may release a staff-only case, even an Administrator's.
        cache_role(603, 2, Permissions::ADMINISTRATOR);
        let target_roles = [Id::new(600), Id::new(603)];
        assert!(
            release_refused(
                &app,
                guild,
                true,
                (Id::new(5), &[Id::new(603)]),
                (user, &target_roles)
            )
            .is_none()
        );
        assert!(
            release_refused(
                &app,
                guild,
                true,
                (Id::new(3), junior),
                (user, &target_roles)
            )
            .is_some()
        );
        assert!(
            release_refused(
                &app,
                guild,
                false,
                (Id::new(3), junior),
                (user, &target_roles)
            )
            .is_none()
        );
    }

    #[tokio::test]
    async fn startup_marks_active_only_the_state_it_listed() {
        let app = crate::app::test_app().await;
        let (guild, user) = (Id::new(77), Id::new(9));
        let record = Record {
            guild_id: guild,
            user_id: user,
            token: "123456".into(),
            roles: Vec::new(),
            state: State::Applying,
            reason: Reason::Scam,
            evidence: String::new(),
            case_message: None,
            created_at: 0,
            kicked: false,
        };
        app.db
            .call(move |c| db::quarantine::insert_new(c, &record))
            .await
            .unwrap();
        let state = |app: Arc<App>| async move {
            app.db
                .call(move |c| db::quarantine::get(c, guild, user))
                .await
                .unwrap()
                .unwrap()
                .state
        };
        // Listed as Releasing, but replaced since by a new, unconfirmed case: left alone.
        let listed = |states: &'static [State], created_at| Listed { states, created_at };
        mark_active(&app, guild, user, listed(&[State::Releasing], 0)).await;
        assert_eq!(state(Arc::clone(&app)).await, State::Applying);
        // Same state, but a different case than the one listed: left alone.
        mark_active(&app, guild, user, listed(&[State::Applying], 5)).await;
        assert_eq!(state(Arc::clone(&app)).await, State::Applying);
        mark_active(&app, guild, user, listed(&[State::Applying], 0)).await;
        assert_eq!(state(Arc::clone(&app)).await, State::Active);
    }

    #[tokio::test]
    async fn buttons_on_an_old_case_card_do_nothing() {
        let app = crate::app::test_app().await;
        let (guild, user) = (Id::new(77), Id::new(9));
        let record = Record {
            guild_id: guild,
            user_id: user,
            token: "123456".into(),
            roles: vec![Id::new(600)],
            state: State::Active,
            reason: Reason::Spam,
            evidence: String::new(),
            case_message: Some((Id::new(5), Id::new(51))),
            created_at: 0,
            kicked: false,
        };
        assert!(is_case_card(&record, Id::new(51)));
        assert!(!is_case_card(&record, Id::new(50)));
        assert!(!is_case_card(
            &Record { case_message: None, ..record.clone() },
            Id::new(51)
        ));
        app.db
            .call(move |c| db::quarantine::insert_new(c, &record))
            .await
            .unwrap();
        let old_card =
            Releaser::Moderator { by: Id::new(3), roles: Vec::new(), card: Some(Id::new(50)) };
        assert!(matches!(
            release(&app, guild, user, old_card)
                .await
                .unwrap(),
            ReleaseOutcome::StaleCard
        ));
        let themselves = Releaser::Moderator { by: user, roles: Vec::new(), card: None };
        assert!(matches!(
            release(&app, guild, user, themselves).await.unwrap(),
            ReleaseOutcome::Failed(reason) if reason.contains("ตัวเอง")
        ));
        assert!(
            app.db
                .call(move |c| db::quarantine::get(c, guild, user))
                .await
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn startup_decides_every_state_role_and_join_combination() {
        use Reconcile::*;
        let case = |state, reason| Record {
            guild_id: Id::new(1),
            user_id: Id::new(2),
            token: String::new(),
            roles: Vec::new(),
            state,
            reason,
            evidence: String::new(),
            case_message: None,
            created_at: 0,
            kicked: false,
        };
        let joins = [None, Some(true), Some(false)];
        for staff in [false, true] {
            for joined in joins {
                let action = |state, holds| {
                    reconcile_action(&case(state, Reason::Spam), staff, holds, joined)
                };
                // Holding the role: a release or re-apply is settled as active.
                assert_eq!(action(State::Active, true), Keep);
                assert_eq!(action(State::Releasing, true), MarkActive);
                assert_eq!(action(State::Reapply, true), MarkActive);
                // An unconfirmed swap is finished whether or not the role is on.
                assert_eq!(action(State::Applying, true), FinishSwap);
                assert_eq!(action(State::Applying, false), FinishSwap);
                assert_eq!(action(State::Reapply, false), Reapply);
                assert_eq!(action(State::Releasing, false), Release);
            }
            let active = case(State::Active, Reason::Spam);
            assert_eq!(
                reconcile_action(&active, staff, false, None),
                Keep
            );
            assert_eq!(
                reconcile_action(&active, staff, false, Some(true)),
                Reapply
            );
            let removed_by_hand = if staff { Reapply } else { Release };
            assert_eq!(
                reconcile_action(&active, staff, false, Some(false)),
                removed_by_hand
            );
        }
    }

    #[test]
    fn startup_releases_only_finished_or_automatic_cases_while_the_role_exists() {
        let case = |state, reason| Record {
            guild_id: Id::new(1),
            user_id: Id::new(2),
            token: String::new(),
            roles: Vec::new(),
            state,
            reason,
            evidence: String::new(),
            case_message: None,
            created_at: 0,
            kicked: false,
        };
        for staff in [false, true] {
            for state in [State::Applying, State::Active, State::Releasing, State::Reapply] {
                assert!(
                    !reconcile_may_release(&case(state, Reason::Spam), staff, false),
                    "{state:?} {staff}"
                );
            }
            assert!(reconcile_may_release(
                &case(State::Releasing, Reason::Spam),
                staff,
                true
            ));
            assert!(!reconcile_may_release(
                &case(State::Applying, Reason::Spam),
                staff,
                true
            ));
            assert!(!reconcile_may_release(
                &case(State::Reapply, Reason::Spam),
                staff,
                true
            ));
        }
        assert!(reconcile_may_release(
            &case(State::Active, Reason::Spam),
            false,
            true
        ));
        assert!(!reconcile_may_release(
            &case(State::Active, Reason::Spam),
            true,
            true
        ));
    }

    #[test]
    fn a_rejoin_after_the_case_opened_is_not_a_manual_release() {
        use twilight_model::util::Timestamp;
        let record = Record {
            guild_id: Id::new(1),
            user_id: Id::new(2),
            token: String::new(),
            roles: Vec::new(),
            state: State::Active,
            reason: Reason::Manual,
            evidence: String::new(),
            case_message: None,
            created_at: 1_000_000,
            kicked: false,
        };
        let at = |secs| Some(Timestamp::from_secs(secs).unwrap());
        assert_eq!(rejoined_since(at(1_000_500), &record), Some(true));
        assert_eq!(rejoined_since(at(900_000), &record), Some(false));
        assert_eq!(rejoined_since(None, &record), None);
    }

    #[test]
    fn tokens_are_six_digits() {
        for _ in 0..50 {
            let token = new_token();
            assert_eq!(token.len(), 6);
            assert!(
                token
                    .chars()
                    .all(|c| c.is_ascii_digit())
            );
        }
    }

    #[tokio::test]
    async fn member_lock_serializes_and_sweeps() {
        let locks = MemberLocks::default();
        let guard = locks.lock(Id::new(1), Id::new(2)).await;
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                locks.lock(Id::new(1), Id::new(2))
            )
            .await
            .is_err()
        );
        drop(guard);
        locks.sweep();
        assert!(locks.locks.lock().unwrap().is_empty());
        assert!(locks.should_remind(Id::new(1), Id::new(2)));
        assert!(!locks.should_remind(Id::new(1), Id::new(2)));
    }

    #[test]
    fn manual_quarantine_respects_rank() {
        let (owner, moderator, target) = (Id::new(1), Id::new(2), Id::new(3));
        let (admin, mod_role, member, helper) =
            (Id::new(10), Id::new(11), Id::new(12), Id::new(13));
        let role = |id: Id<RoleMarker>| match id.get() {
            10 => Some((9, true)),
            11 => Some((5, false)),
            12 => Some((2, false)),
            13 => Some((5, false)),
            _ => None,
        };
        let check = |moderator_id, moderator_roles: &[_], target_id, target_roles: &[_]| {
            outranks(
                Some(owner),
                (moderator_id, moderator_roles),
                (target_id, target_roles),
                role,
            )
        };
        assert!(check(moderator, &[mod_role], target, &[member]).is_ok());
        assert!(check(moderator, &[mod_role], target, &[]).is_ok());
        assert!(check(moderator, &[mod_role], owner, &[]).is_err());
        assert!(check(moderator, &[mod_role], target, &[admin]).is_err());
        assert!(check(moderator, &[mod_role], target, &[helper]).is_err());
        assert!(check(moderator, &[member], target, &[mod_role]).is_err());
        assert!(check(owner, &[], target, &[admin]).is_ok());
    }
}
