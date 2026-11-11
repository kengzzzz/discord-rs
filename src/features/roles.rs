use std::{
    hash::{DefaultHasher, Hash as _, Hasher as _},
    sync::Arc,
};

use twilight_http::request::AuditLogReason as _;
use twilight_model::{
    application::interaction::Interaction,
    channel::message::{Component, component::ButtonStyle},
    guild::Permissions,
    id::{
        Id,
        marker::{GuildMarker, MessageMarker, RoleMarker, UserMarker},
    },
};

use crate::{
    app::App,
    db::{
        self,
        messages::{BotMessage, BotMessageKind},
    },
    discord::{
        errors, reply,
        ui::{self, Card, color},
    },
    guild::{ChannelKind, RoleKind},
};

pub const PREFIX: &str = "role:";

/// Roles the bot hands out can end up on anyone, so none may carry these.
const STAFF_PERMISSIONS: Permissions = Permissions::ADMINISTRATOR
    .union(Permissions::MANAGE_GUILD)
    .union(Permissions::MANAGE_ROLES)
    .union(Permissions::MANAGE_CHANNELS)
    .union(Permissions::MANAGE_MESSAGES)
    .union(Permissions::MANAGE_THREADS)
    .union(Permissions::MANAGE_WEBHOOKS)
    .union(Permissions::MANAGE_NICKNAMES)
    .union(Permissions::MANAGE_EVENTS)
    .union(Permissions::MANAGE_GUILD_EXPRESSIONS)
    .union(Permissions::KICK_MEMBERS)
    .union(Permissions::BAN_MEMBERS)
    .union(Permissions::MODERATE_MEMBERS)
    .union(Permissions::MUTE_MEMBERS)
    .union(Permissions::DEAFEN_MEMBERS)
    .union(Permissions::MOVE_MEMBERS)
    .union(Permissions::MENTION_EVERYONE);

/// Only powers beyond @everyone's count. An uncached role counts as staff.
pub fn grants_staff(app: &App, guild: Id<GuildMarker>, role: Id<RoleMarker>) -> bool {
    let everyone = app
        .cache
        .role(guild.cast())
        .map_or(Permissions::empty(), |r| r.permissions);
    app.cache
        .role(role)
        .is_none_or(|r| (r.permissions - everyone).intersects(STAFF_PERMISSIONS))
}

pub fn existing_role_grants_staff(app: &App, guild: Id<GuildMarker>, role: Id<RoleMarker>) -> bool {
    app.cache.role(role).is_some() && grants_staff(app, guild, role)
}

pub fn render(app: &App, guild: Id<GuildMarker>) -> Option<Vec<Component>> {
    let config = app.guild(guild);
    let mut card = Card::new(color::BRAND)
        .title("เลือก Role ที่สนใจ")
        .text("กดเพื่อรับ · กดอีกครั้งเพื่อเอาออก")
        .divider();
    let mut any = false;
    for kind in RoleKind::PICKABLE {
        let Some(binding) = config.role(kind).filter(|b| {
            b.self_assignable
                && crate::moderation::quarantine::role_exists(app, guild, b.id)
                && !existing_role_grants_staff(app, guild, b.id)
        }) else {
            continue;
        };
        let name = app
            .cache
            .role(binding.id)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| kind.label().to_owned());
        any = true;
        card = card.section_button(
            format!(
                "**{}**\n-# {}",
                ui::escape_markdown(&name),
                kind.hint()
            ),
            ui::button_with(
                ButtonStyle::Secondary,
                format!("{PREFIX}{}", kind.as_str()),
                "รับ / เอาออก",
                false,
            ),
        );
    }
    if !any {
        return None;
    }
    Some(vec![card.build()])
}

fn digest(components: &[Component]) -> u64 {
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(components)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

pub async fn ensure_picker(app: &Arc<App>, guild: Id<GuildMarker>) {
    // Uncached roles would show internal names; GuildCreate runs this again.
    if app.cache.guild_roles(guild).is_none() {
        return;
    }
    let result = try_ensure(app, guild).await;
    let mut retry = app
        .picker_retry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match result {
        Ok(()) => {
            if retry.remove(&guild) {
                tracing::info!(guild = guild.get(), "role picker recovered");
            }
        }
        // A 4xx is a setup problem the admin fixes in the panel; the rest retry every minute.
        Err(error)
            if error
                .downcast_ref::<twilight_http::Error>()
                .is_some_and(errors::refused) =>
        {
            retry.remove(&guild);
            tracing::warn!(guild = guild.get(), %error, "failed to ensure role picker");
        }
        Err(error) => {
            if retry.insert(guild) {
                tracing::warn!(guild = guild.get(), %error, "failed to ensure role picker; retrying every minute");
            }
        }
    }
}

pub fn start(app: &Arc<App>) {
    let app2 = Arc::clone(app);
    app.spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = app2.shutdown.cancelled() => break,
                _ = tick.tick() => {}
            }
            let pending: Vec<_> = app2
                .picker_retry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .copied()
                .collect();
            for guild in pending {
                if app2.shutdown.is_cancelled() {
                    break;
                }
                ensure_picker(&app2, guild).await;
            }
        }
    });
}

async fn try_ensure(app: &Arc<App>, guild: Id<GuildMarker>) -> anyhow::Result<()> {
    let _guard = app.bot_messages.lock().await;
    let stored = app
        .db
        .call(move |c| db::messages::get(c, guild, BotMessageKind::RolePicker))
        .await?;
    let channel = app
        .guild(guild)
        .channel(ChannelKind::RolePicker);
    let components = channel.and_then(|_| render(app, guild));

    let (Some(channel), Some(components)) = (channel, components) else {
        if let Some(old) = stored {
            delete_quietly(app, old).await;
            app.db
                .call(move |c| db::messages::delete(c, guild, BotMessageKind::RolePicker))
                .await?;
        }
        return Ok(());
    };
    let digest = digest(&components);

    if let Some(old) = stored {
        // Still check it exists: a delete while offline sent no event.
        if old.channel_id == channel && old.digest == Some(digest) {
            match app
                .http
                .message(channel, old.message_id)
                .await
            {
                Ok(_) => return Ok(()),
                Err(error) if errors::is(&error, errors::UNKNOWN_MESSAGE) => {}
                // Without Read Message History the check is impossible; trust the record.
                Err(error) if errors::refused(&error) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
        if old.channel_id == channel && old.digest.is_some() {
            match app
                .http
                .update_message(channel, old.message_id)
                .components(Some(&components))
                .await
            {
                Ok(_) => {
                    let record = BotMessage { digest: Some(digest), ..old };
                    app.db
                        .call(move |c| {
                            db::messages::set(c, guild, BotMessageKind::RolePicker, record)
                        })
                        .await?;
                    return Ok(());
                }
                Err(error) if errors::is(&error, errors::UNKNOWN_MESSAGE) => {}
                Err(error) => return Err(error.into()),
            }
        } else {
            delete_quietly(app, old).await;
        }
    }

    let message =
        crate::discord::post_card(app, guild, "role_picker", channel, &components).await?;
    let record = BotMessage { channel_id: channel, message_id: message.id, digest: Some(digest) };
    app.db
        .call(move |c| db::messages::set(c, guild, BotMessageKind::RolePicker, record))
        .await?;
    Ok(())
}

async fn delete_quietly(app: &App, message: BotMessage) {
    if let Err(error) = app
        .http
        .delete_message(message.channel_id, message.message_id)
        .await
        && !errors::is(&error, errors::UNKNOWN_MESSAGE)
    {
        tracing::debug!(%error, "could not delete previous bot message");
    }
}

pub async fn on_deleted(app: &Arc<App>, guild: Id<GuildMarker>, ids: &[Id<MessageMarker>]) {
    // See `status::on_deleted`.
    let posting = app.bot_messages.try_lock().is_err();
    if !posting
        && !db::messages::stored_among(&app.db, guild, BotMessageKind::RolePicker, ids).await
    {
        return;
    }
    let forgotten = {
        // Don't let a stuck card post hold every delete event; the next update finds a missed card.
        let Ok(_guard) = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            app.bot_messages.lock(),
        )
        .await
        else {
            tracing::warn!(
                guild = guild.get(),
                "card lock busy; skipping a delete check"
            );
            return;
        };
        match app
            .db
            .call(move |c| db::messages::get(c, guild, BotMessageKind::RolePicker))
            .await
        {
            Ok(Some(record)) if ids.contains(&record.message_id) => app
                .db
                .call(move |c| db::messages::delete(c, guild, BotMessageKind::RolePicker))
                .await
                .is_ok(),
            _ => false,
        }
    };
    if forgotten {
        ensure_picker(app, guild).await;
    }
}

const TOGGLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub async fn on_button(
    app: &Arc<App>,
    interaction: &Interaction,
    custom_id: &str,
) -> anyhow::Result<()> {
    let (Some(guild), Some(member)) = (interaction.guild_id, interaction.member.as_ref()) else {
        return Ok(());
    };
    let Some(user) = member.user.as_ref() else {
        return Ok(());
    };
    let Some(kind) = custom_id
        .strip_prefix(PREFIX)
        .and_then(RoleKind::parse)
    else {
        return Ok(());
    };
    reply::defer(app, interaction, true).await?;
    let card = toggle(app, guild, user.id, &member.roles, kind).await?;
    reply::edit(app, interaction, &card).await?;
    Ok(())
}

async fn toggle(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    roles: &[Id<RoleMarker>],
    kind: RoleKind,
) -> anyhow::Result<Vec<Component>> {
    let Some(binding) = app
        .guild(guild)
        .role(kind)
        .filter(|b| b.self_assignable)
    else {
        return Ok(ui::error("role นี้ไม่ได้เปิดให้เลือกแล้ว"));
    };
    // Right after a reconnect every role would count as staff below.
    if app.cache.guild_roles(guild).is_none() {
        return Ok(ui::error("ลองใหม่อีกครั้ง"));
    }
    if grants_staff(app, guild, binding.id) {
        tracing::warn!(
            guild = guild.get(),
            role = binding.id.get(),
            "picker role has staff permissions; refusing"
        );
        return Ok(ui::error("role นี้เลือกเองไม่ได้ แจ้งแอดมิน"));
    }
    let _guard = app
        .moderation
        .locks
        .lock(guild, user)
        .await;
    if let Some(record) = app
        .db
        .call(move |c| db::quarantine::get(c, guild, user))
        .await?
    {
        return Ok(ui::notice(
            color::WARNING,
            "ยังเลือก role ไม่ได้",
            crate::moderation::quarantine::blocked_text(app, &record),
        ));
    }

    let has = roles.contains(&binding.id);
    // Don't hold the interaction place through a long role queue.
    let call = async {
        if has {
            app.http
                .remove_guild_member_role(guild, user, binding.id)
                .reason("role picker")
                .await
        } else {
            app.http
                .add_guild_member_role(guild, user, binding.id)
                .reason("role picker")
                .await
        }
    };
    let Ok(result) = tokio::time::timeout(TOGGLE_TIMEOUT, call).await else {
        // The call may still land, so don't say it failed.
        return Ok(ui::notice(
            color::WARNING,
            "Discord ยังไม่ตอบ",
            "ดู role ของคุณก่อนกดอีกครั้ง",
        ));
    };
    let role = format!("<@&{}>", binding.id);
    Ok(match result {
        Ok(_) if has => ui::notice(color::MUTED, "เอา role ออกแล้ว", &role),
        Ok(_) => ui::success("ได้รับ role แล้ว", &role),
        Err(error) if errors::is(&error, errors::MISSING_PERMISSIONS) => {
            ui::error("บอทจัดการ role นี้ไม่ได้ แจ้งแอดมิน")
        }
        Err(error) => {
            tracing::warn!(%error, "role toggle failed");
            ui::error("เปลี่ยน role ไม่สำเร็จ ลองใหม่อีกครั้ง")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_guild_leaves_the_retry_list_once_its_picker_is_fine() {
        let app = crate::app::test_app().await;
        let guild = Id::new(77);
        app.picker_retry
            .lock()
            .unwrap()
            .insert(guild);
        // Roles not cached yet: nothing is checked, so the guild stays listed.
        ensure_picker(&app, guild).await;
        assert!(
            app.picker_retry
                .lock()
                .unwrap()
                .contains(&guild)
        );
        let everyone = serde_json::from_value(serde_json::json!({
            "id": "77", "name": "@everyone", "color": 0, "hoist": false,
            "colors": {"primary_color": 0}, "position": 0, "permissions": "0",
            "managed": false, "mentionable": false, "flags": 0
        }))
        .unwrap();
        app.cache.update(
            &twilight_model::gateway::event::Event::RoleCreate(
                twilight_model::gateway::payload::incoming::RoleCreate {
                    guild_id: guild,
                    role: everyone,
                },
            ),
        );
        // No picker channel configured: nothing to post, which counts as success.
        ensure_picker(&app, guild).await;
        assert!(
            app.picker_retry
                .lock()
                .unwrap()
                .is_empty()
        );
    }
    use crate::{app::test_app, discord::ui::validate_message, guild::RoleBinding};

    #[tokio::test]
    async fn picker_with_every_role_fits_limits_and_hides_unpickable() {
        let app = test_app().await;
        let guild = Id::new(5);
        assert!(render(&app, guild).is_none());
        for (index, kind) in RoleKind::ALL.into_iter().enumerate() {
            let binding = RoleBinding {
                id: Id::new(900 + index as u64),
                self_assignable: RoleKind::PICKABLE.contains(&kind),
            };
            app.guilds
                .set_role(guild, kind, binding)
                .await
                .unwrap();
        }
        let components = render(&app, guild).unwrap();
        validate_message(&components).unwrap();
        let json = serde_json::to_string(&components).unwrap();
        assert!(json.contains("role:baro") && !json.contains("role:guest"));
    }
}
