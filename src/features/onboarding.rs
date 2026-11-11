use std::sync::Arc;

use twilight_http::request::AuditLogReason as _;
use twilight_model::{
    application::interaction::{Interaction, modal::ModalInteractionData},
    channel::message::{
        Component, MessageFlags,
        component::{TextInput, TextInputStyle},
    },
    id::{
        Id,
        marker::{GuildMarker, MessageMarker, UserMarker},
    },
};
use twilight_util::builder::message::LabelBuilder;

use crate::{
    app::{self, App},
    db,
    discord::{
        self, reply,
        ui::{self, Card, color},
    },
    guild::{ChannelKind, RoleKind},
    moderation::quarantine::{self, ReleaseOutcome, Releaser},
};

pub const INTRO_MODAL: &str = "intro";
pub const VERIFY_MODAL: &str = "verify";

#[derive(Debug, PartialEq, Eq)]
pub struct Intro {
    pub name: String,
    pub age: Option<u8>,
    pub ign: Option<String>,
    pub clan: Option<String>,
    pub about: Option<String>,
}

fn clean(value: Option<&str>, max: usize) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.chars().take(max).collect())
}

pub fn parse_intro(data: &ModalInteractionData) -> Option<Intro> {
    Some(Intro {
        name: clean(discord::modal_value(data, "name"), 100)?,
        age: discord::modal_value(data, "age")
            .and_then(|v| v.trim().parse().ok())
            .filter(|age| (1..=120).contains(age)),
        ign: clean(discord::modal_value(data, "ign"), 50),
        clan: clean(discord::modal_value(data, "clan"), 50),
        about: clean(discord::modal_value(data, "about"), 300),
    })
}

#[allow(deprecated)]
fn input(
    custom_id: &str,
    label: &str,
    placeholder: &str,
    required: bool,
    max: u16,
    style: TextInputStyle,
) -> Component {
    LabelBuilder::new(
        label,
        Component::TextInput(TextInput {
            custom_id: custom_id.to_owned(),
            id: None,
            label: None,
            max_length: Some(max),
            min_length: required.then_some(1),
            placeholder: Some(placeholder.to_owned()),
            required: Some(required),
            style,
            value: None,
        }),
    )
    .build()
    .into()
}

/// Queued as ids for 16 workers, so a join raid can't hold a task per join. Past the cap a join
/// gets no Guest role or welcome.
pub fn queue_join(app: &Arc<App>, guild: Id<GuildMarker>, user: Id<UserMarker>) {
    static DROPPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let (start, dropped) = app.joins.push([(guild, user)], false);
    if dropped > 0 {
        metrics::counter!("joins_dropped_total").increment(1);
        if !DROPPING.swap(true, std::sync::atomic::Ordering::Relaxed) {
            tracing::warn!(
                guild = guild.get(),
                "too many joins waiting; dropping joins (no Guest role or welcome)"
            );
        }
    } else if DROPPING.swap(false, std::sync::atomic::Ordering::Relaxed) {
        tracing::info!("joins caught up");
    }
    for _ in 0..start {
        let app2 = Arc::clone(app);
        app.spawn(async move {
            let mut worker = app2.joins.worker();
            while let Some((guild, user)) = worker.take() {
                if app2.shutdown.is_cancelled() {
                    break;
                }
                Box::pin(on_join(&app2, guild, user)).await;
            }
        });
    }
}

async fn on_join(app: &Arc<App>, guild: Id<GuildMarker>, user_id: Id<UserMarker>) {
    let config = app.guild(guild);
    let _guard = app
        .moderation
        .locks
        .lock(guild, user_id)
        .await;
    let record = match app
        .db
        .call(move |c| db::quarantine::get(c, guild, user_id))
        .await
    {
        Ok(record) => record,
        // Granting Guest to someone who may be quarantined is worse than skipping the welcome.
        Err(error) => {
            tracing::error!(guild = guild.get(), user = user_id.get(), %error, "quarantine lookup failed on join");
            return;
        }
    };

    if let Some(record) = record {
        let Some(role) = config.role_id(RoleKind::Quarantine) else {
            tracing::warn!(
                guild = guild.get(),
                user = user_id.get(),
                "quarantined member rejoined but no quarantine role is set"
            );
            return;
        };
        if super::roles::grants_staff(app, guild, role) {
            tracing::error!(
                guild = guild.get(),
                role = role.get(),
                "quarantine role has staff permissions; not re-applying it"
            );
        } else if let Err(error) = app
            .http
            .add_guild_member_role(guild, user_id, role)
            .reason("re-applying quarantine")
            .await
        {
            // Startup reconcile re-applies it too.
            tracing::error!(guild = guild.get(), user = user_id.get(), %error, "failed to re-apply quarantine role on join");
        }
        if let Some(channel) = config.channel(ChannelKind::Quarantine)
            && app
                .moderation
                .locks
                .should_remind(guild, user_id)
        {
            let card = quarantine::reminder_card(app, guild, channel, &record);
            let (app2, user) = (Arc::clone(app), user_id);
            app.spawn_outbound("join_reminder", async move {
                discord::dm_or_channel(&app2, user, Some(channel), &card, true).await;
            });
        }
        return;
    }
    // During a raid the join can be handled after the member already ran `/intro`.
    if intro_while_here(guild, user_id) {
        return;
    }

    let (Some(guest), Some(intro)) = (
        config.role_id(RoleKind::Guest),
        config.channel(ChannelKind::Introduction),
    ) else {
        return;
    };
    // Imported bindings skipped the panel's checks.
    if super::roles::grants_staff(app, guild, guest) {
        tracing::warn!(
            guild = guild.get(),
            role = guest.get(),
            "guest role has staff permissions; not granting it"
        );
    } else {
        // Holds the member lock and a join permit, so don't wait out a long role queue.
        match tokio::time::timeout(
            GRANT_TIMEOUT,
            app.http
                .add_guild_member_role(guild, user_id, guest)
                .reason("new member")
                .into_future(),
        )
        .await
        {
            Ok(Ok(_)) => {}
            // Left or was banned while waiting; skip the welcome.
            Ok(Err(error)) if discord::errors::is(&error, discord::errors::UNKNOWN_MEMBER) => {
                return;
            }
            Ok(Err(error)) => tracing::warn!(%error, "failed to assign guest role"),
            Err(_) => tracing::warn!(
                guild = guild.get(),
                user = user_id.get(),
                "guest role grant timed out"
            ),
        }
    }
    let card = Card::new(color::BRAND)
        .section(
            format!(
                "### ยินดีต้อนรับสู่ {}\nพิมพ์ `/intro` ใน <#{intro}> เพื่อรับสิทธิ์สมาชิก",
                app.guild_name(guild)
            ),
            app.guild_icon(guild).as_deref(),
        )
        .small("ทุกคนจะเห็นข้อมูลที่กรอก")
        .build();
    let (app2, user) = (Arc::clone(app), user_id);
    app.spawn_outbound("welcome", async move {
        discord::dm_or_channel(&app2, user, Some(intro), &[card], true).await;
    });
}

const GRANT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const INTRO_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(10 * 60);

struct IntroCard {
    posted: std::time::Instant,
    /// `None` when the post's response couldn't be read; taken as still up.
    message: Option<Id<MessageMarker>>,
    deleted: bool,
    left: bool,
}

type IntroCards = std::collections::HashMap<(Id<GuildMarker>, Id<UserMarker>), IntroCard>;
static LAST_INTRO: std::sync::LazyLock<std::sync::Mutex<IntroCards>> =
    std::sync::LazyLock::new(std::sync::Mutex::default);

fn intro_cards() -> std::sync::MutexGuard<'static, IntroCards> {
    let mut cards = LAST_INTRO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = std::time::Instant::now();
    cards.retain(|_, card| now.duration_since(card.posted) < INTRO_COOLDOWN);
    cards
}

fn intro_while_here(guild: Id<GuildMarker>, user: Id<UserMarker>) -> bool {
    intro_cards()
        .get(&(guild, user))
        .is_some_and(|card| !card.left)
}

fn intro_card_up(guild: Id<GuildMarker>, user: Id<UserMarker>) -> bool {
    intro_cards()
        .get(&(guild, user))
        .is_some_and(|card| !card.deleted)
}

pub fn on_leave(guild: Id<GuildMarker>, user: Id<UserMarker>) {
    if let Some(card) = intro_cards().get_mut(&(guild, user)) {
        card.left = true;
    }
}

/// After staff delete a card, a rejoining member posts a new one.
pub fn on_deleted(guild: Id<GuildMarker>, ids: &[Id<MessageMarker>]) {
    for ((g, _), card) in intro_cards().iter_mut() {
        if *g == guild
            && card
                .message
                .is_some_and(|m| ids.contains(&m))
        {
            card.deleted = true;
        }
    }
}

/// A rejoin that got Member back without a new card; the cooldown runs from the first post.
fn mark_here(guild: Id<GuildMarker>, user: Id<UserMarker>) {
    if let Some(card) = intro_cards().get_mut(&(guild, user)) {
        card.left = false;
    }
}

fn record_intro(guild: Id<GuildMarker>, user: Id<UserMarker>, message: Option<Id<MessageMarker>>) {
    intro_cards().insert(
        (guild, user),
        IntroCard { posted: std::time::Instant::now(), message, deleted: false, left: false },
    );
}

pub async fn open_intro(app: &Arc<App>, interaction: &Interaction) -> anyhow::Result<()> {
    let Some(guild) = interaction.guild_id else {
        return Ok(());
    };
    if app
        .guild(guild)
        .channel(ChannelKind::Introduction)
        .is_none()
    {
        return reply::respond(
            app,
            interaction,
            ui::error("เซิร์ฟเวอร์นี้ยังไม่ได้ตั้งค่าช่องแนะนำตัว"),
            true,
        )
        .await;
    }
    if let Some(user) = interaction.author_id() {
        let record = app
            .db
            .call(move |c| db::quarantine::get(c, guild, user))
            .await?;
        if let Some(record) = record {
            return reply::respond(
                app,
                interaction,
                ui::error(quarantine::blocked_text(app, &record)),
                true,
            )
            .await;
        }
    }
    let components = vec![
        input(
            "name",
            "ชื่อเล่น",
            "อยากให้เพื่อน ๆ เรียกว่าอะไร",
            true,
            100,
            TextInputStyle::Short,
        ),
        input(
            "ign",
            "ชื่อในเกม (IGN)",
            "ไม่บังคับ",
            false,
            50,
            TextInputStyle::Short,
        ),
        input(
            "clan",
            "แคลน",
            "ไม่บังคับ",
            false,
            50,
            TextInputStyle::Short,
        ),
        input(
            "age",
            "อายุ",
            "ไม่บังคับ",
            false,
            3,
            TextInputStyle::Short,
        ),
        input(
            "about",
            "เกี่ยวกับตัวเอง",
            "เล่นมานานแค่ไหน ชอบเฟรมไหน (ไม่บังคับ)",
            false,
            300,
            TextInputStyle::Paragraph,
        ),
    ];
    reply::modal(
        app,
        interaction,
        INTRO_MODAL,
        "แนะนำตัว",
        components,
    )
    .await
}

pub async fn submit_intro(
    app: &Arc<App>,
    interaction: &Interaction,
    data: &ModalInteractionData,
) -> anyhow::Result<()> {
    let (Some(guild), Some(member)) = (interaction.guild_id, interaction.member.as_ref()) else {
        return Ok(());
    };
    let Some(user) = member.user.as_ref() else {
        return Ok(());
    };
    reply::defer(app, interaction, true).await?;
    let config = app.guild(guild);
    let Some(intro_channel) = config.channel(ChannelKind::Introduction) else {
        reply::edit(
            app,
            interaction,
            &ui::error("เซิร์ฟเวอร์นี้ยังไม่ได้ตั้งค่าช่องแนะนำตัว"),
        )
        .await?;
        return Ok(());
    };
    let Some(intro) = parse_intro(data) else {
        reply::edit(app, interaction, &ui::error("กรอกชื่อเล่นก่อน")).await?;
        return Ok(());
    };
    let user_id = user.id;
    // Posted as the bot, which skips spam and scam checks, so intros are checked here.
    let all_text = [
        Some(intro.name.as_str()),
        intro.ign.as_deref(),
        intro.clan.as_deref(),
        intro.about.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n");
    let scam = &app.config.scam;
    if crate::moderation::scoring::score_text(
        &all_text,
        scam.block_threshold,
        scam.review_threshold,
    )
    .action
        == crate::moderation::scoring::Action::Block
    {
        tracing::warn!(
            guild = guild.get(),
            user = user_id.get(),
            "intro text looks like a scam; not posted"
        );
        reply::edit(
            app,
            interaction,
            &ui::error("ข้อความนี้ดูเหมือนสแปม แก้แล้วลองใหม่"),
        )
        .await?;
        return Ok(());
    }
    // Held until the roles change, so a quarantine can't land between check and grant.
    let _guard = app
        .moderation
        .locks
        .lock(guild, user_id)
        .await;
    // Under the lock, so two submits can't both pass. A rejoin gets Member back, but the card
    // posts at most once in 10 minutes unless staff deleted it.
    if intro_while_here(guild, user_id) {
        reply::edit(
            app,
            interaction,
            &ui::error("แนะนำตัวไปแล้ว ลองใหม่ในอีก 10 นาที"),
        )
        .await?;
        return Ok(());
    }
    let card_up = intro_card_up(guild, user_id);
    if let Some(record) = app
        .db
        .call(move |c| db::quarantine::get(c, guild, user_id))
        .await?
    {
        reply::edit(
            app,
            interaction,
            &ui::error(quarantine::blocked_text(app, &record)),
        )
        .await?;
        return Ok(());
    }

    if card_up
        && config
            .role_id(RoleKind::Member)
            .is_none()
    {
        reply::edit(
            app,
            interaction,
            &ui::error("role สมาชิกตั้งค่าไม่ถูกต้อง แจ้งแอดมิน"),
        )
        .await?;
        return Ok(());
    }
    if let Some(role) = config.role_id(RoleKind::Member)
        && super::roles::grants_staff(app, guild, role)
    {
        tracing::warn!(
            guild = guild.get(),
            role = role.get(),
            "member role has staff permissions; not granting it"
        );
        reply::edit(
            app,
            interaction,
            &ui::error("role สมาชิกตั้งค่าไม่ถูกต้อง แจ้งแอดมิน"),
        )
        .await?;
        return Ok(());
    }
    if let Some(role) = config.role_id(RoleKind::Member)
        && !member.roles.contains(&role)
        && let Err(error) = app
            .http
            .add_guild_member_role(guild, user.id, role)
            .reason("introduced")
            .await
    {
        tracing::warn!(%error, "failed to grant member role");
        reply::edit(
            app,
            interaction,
            &ui::error("ให้ role สมาชิกไม่สำเร็จ แจ้งแอดมิน"),
        )
        .await?;
        return Ok(());
    }
    if let Some(role) = config.role_id(RoleKind::Guest)
        && member.roles.contains(&role)
        && let Err(error) = app
            .http
            .remove_guild_member_role(guild, user.id, role)
            .reason("introduced")
            .await
    {
        tracing::warn!(%error, "failed to remove guest role");
    }
    if card_up {
        mark_here(guild, user_id);
        reply::edit(
            app,
            interaction,
            &ui::success("ได้ role สมาชิกคืนแล้ว", "การ์ดแนะนำตัวเดิมยังอยู่"),
        )
        .await?;
        return Ok(());
    }

    let mut lines = vec![format!("**ชื่อ** {}", ui::escape_markdown(&intro.name))];
    if let Some(ign) = &intro.ign {
        lines.push(format!("**IGN** {}", ui::escape_markdown(ign)));
    }
    if let Some(clan) = &intro.clan {
        lines.push(format!("**แคลน** {}", ui::escape_markdown(clan)));
    }
    if let Some(age) = intro.age {
        lines.push(format!("**อายุ** {age}"));
    }
    let now = jiff::Timestamp::now().as_millisecond();
    let mut card = Card::new(color::BRAND).section(
        format!(
            "### {} แนะนำตัว\n{}",
            ui::escape_markdown(app::display_name(user)),
            lines.join("\n")
        ),
        Some(&app::avatar_url(user)),
    );
    if let Some(about) = &intro.about {
        card = card.text(format!(">>> {}", ui::escape_markdown(about)));
    }
    let card = card
        .small(format!("<@{}> · {}", user.id, ui::ts(now, 'R')))
        .build();
    let posted = app
        .http
        .create_message(intro_channel)
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(&[card])
        .await?;
    let message = match discord::read(posted).await {
        Ok(message) => Some(message.id),
        Err(error) => {
            tracing::warn!(%error, "intro card posted but its id is unknown");
            None
        }
    };
    record_intro(guild, user_id, message);

    let welcome = Card::new(color::SUCCESS)
        .section(
            format!(
                "### ยินดีต้อนรับ {}\nคุณเป็นสมาชิก **{}** แล้ว",
                ui::escape_markdown(&intro.name),
                ui::escape_markdown(&app.guild_name(guild))
            ),
            app.guild_icon(guild).as_deref(),
        )
        .build();
    discord::dm_or_channel(app, user.id, None, &[welcome], false).await;
    reply::edit(
        app,
        interaction,
        &ui::success(
            "แนะนำตัวสำเร็จ",
            &format!("โพสต์แล้วที่ <#{intro_channel}>"),
        ),
    )
    .await?;
    Ok(())
}

pub async fn open_verify(app: &Arc<App>, interaction: &Interaction) -> anyhow::Result<()> {
    let (Some(guild), Some(user)) = (interaction.guild_id, interaction.author_id()) else {
        return Ok(());
    };
    let Some(record) = app
        .db
        .call(move |c| db::quarantine::get(c, guild, user))
        .await?
    else {
        return reply::respond(
            app,
            interaction,
            ui::notice(color::MUTED, "ไม่ต้องยืนยัน", "คุณไม่ได้ถูกกักกัน"),
            true,
        )
        .await;
    };
    let Some(token) = quarantine::verify_token(app, &record).map(str::to_owned) else {
        return reply::respond(app, interaction, staff_only_card(), true).await;
    };
    #[allow(deprecated)]
    let token = TextInput {
        custom_id: "token".to_owned(),
        id: None,
        label: None,
        max_length: Some(12),
        min_length: Some(1),
        placeholder: None,
        required: Some(true),
        style: TextInputStyle::Short,
        value: Some(token),
    };
    let components = vec![
        LabelBuilder::new("Token ยืนยันตัวตน", Component::TextInput(token))
            .build()
            .into(),
    ];
    reply::modal(
        app,
        interaction,
        VERIFY_MODAL,
        "ยืนยันว่าคุณไม่ใช่บอท",
        components,
    )
    .await
}

fn staff_only_card() -> Vec<Component> {
    ui::notice(
        color::WARNING,
        "ยืนยันเองไม่ได้",
        "ทีมงานกักกันคุณไว้ รอทีมงานปล่อย",
    )
}

pub async fn submit_verify(
    app: &Arc<App>,
    interaction: &Interaction,
    data: &ModalInteractionData,
) -> anyhow::Result<()> {
    let (Some(guild), Some(user)) = (interaction.guild_id, interaction.author_id()) else {
        return Ok(());
    };
    reply::defer(app, interaction, true).await?;
    let token = discord::modal_value(data, "token")
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let card = match quarantine::release(app, guild, user, Releaser::Token(token)).await {
        Ok(ReleaseOutcome::Released) => ui::success("ยืนยันแล้ว", "คืน role ให้แล้ว"),
        Ok(ReleaseOutcome::NotQuarantined | ReleaseOutcome::ClosedGone) => {
            ui::notice(color::MUTED, "ไม่ต้องยืนยัน", "คุณไม่ได้ถูกกักกัน")
        }
        Ok(ReleaseOutcome::WrongToken) => ui::error("Token ไม่ถูกต้อง"),
        Ok(ReleaseOutcome::StaffOnly) => staff_only_card(),
        Ok(ReleaseOutcome::StaleCard) => ui::error("Token ไม่ถูกต้อง"),
        Ok(ReleaseOutcome::Failed(reason)) => {
            ui::error(&format!("คืน role ไม่สำเร็จ ({reason}) แจ้งทีมงาน"))
        }
        Err(error) => {
            tracing::error!(%error, "verify failed");
            ui::error("ระบบขัดข้อง ลองใหม่ภายหลัง")
        }
    };
    reply::edit(app, interaction, &card).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use twilight_model::application::interaction::modal::{
        ModalInteractionComponent, ModalInteractionLabel, ModalInteractionTextInput,
    };

    #[test]
    fn a_late_join_after_intro_is_skipped_unless_the_member_left() {
        let (guild, user) = (Id::new(9001), Id::new(9002));
        assert!(!intro_while_here(guild, user));
        record_intro(guild, user, Some(Id::new(7)));
        assert!(intro_while_here(guild, user));
        on_leave(guild, user);
        assert!(!intro_while_here(guild, user));
        assert!(intro_card_up(guild, user));
        mark_here(guild, user);
        assert!(intro_while_here(guild, user));
        on_deleted(Id::new(1), &[Id::new(7)]);
        assert!(intro_card_up(guild, user));
        on_deleted(guild, &[Id::new(6), Id::new(7)]);
        assert!(!intro_card_up(guild, user));
        assert!(intro_while_here(guild, user));
    }

    fn data(fields: &[(&str, &str)]) -> ModalInteractionData {
        ModalInteractionData {
            custom_id: INTRO_MODAL.into(),
            components: fields
                .iter()
                .map(|(id, value)| {
                    ModalInteractionComponent::Label(ModalInteractionLabel {
                        id: 0,
                        component: Box::new(ModalInteractionComponent::TextInput(
                            ModalInteractionTextInput {
                                custom_id: (*id).into(),
                                id: 0,
                                value: (*value).into(),
                            },
                        )),
                    })
                })
                .collect(),
            resolved: None,
        }
    }

    #[test]
    fn intro_requires_name_and_trims_optional_fields() {
        assert_eq!(parse_intro(&data(&[("name", "  ")])), None);
        let parsed = parse_intro(&data(&[
            ("name", " Keng "),
            ("age", "200"),
            ("ign", " "),
            ("clan", "Tenno"),
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            Intro {
                name: "Keng".into(),
                age: None,
                ign: None,
                clan: Some("Tenno".into()),
                about: None
            }
        );
    }
}
