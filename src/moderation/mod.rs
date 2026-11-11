pub mod ocr;
pub mod quarantine;
pub mod scam;
pub mod scoring;
pub mod spam;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use twilight_model::{
    channel::{Message, message::MessageType},
    guild::Permissions,
    id::{
        Id,
        marker::{GuildMarker, RoleMarker, UserMarker},
    },
};

use crate::{
    app::App,
    config::ScamConfig,
    db::{self, quarantine::Reason},
    discord::errors,
    features::relay,
    guild::{ChannelKind, RoleKind},
};

use self::{
    quarantine::{Evidence, MemberLocks},
    scam::ScamQueue,
    spam::{Observed, SpamDetector, Verdict},
};

pub struct Moderation {
    pub spam: SpamDetector,
    pub locks: MemberLocks,
    pub scam: ScamQueue,
    /// Shared by quarantines, exclusive while the Quarantine role setting changes.
    pub quarantine_config: tokio::sync::RwLock<()>,
}

impl Moderation {
    pub fn new(config: &ScamConfig) -> anyhow::Result<Self> {
        Ok(Self {
            spam: SpamDetector::default(),
            locks: MemberLocks::default(),
            scam: ScamQueue::new(config)?,
            quarantine_config: tokio::sync::RwLock::new(()),
        })
    }
}

pub fn start(app: &Arc<App>) {
    scam::start(app);
    let app2 = Arc::clone(app);
    app.spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(120));
        loop {
            tokio::select! {
                () = app2.shutdown.cancelled() => break,
                _ = tick.tick() => {
                    app2.moderation.spam.sweep(Instant::now());
                    app2.moderation.locks.sweep();
                }
            }
        }
    });
}

pub async fn on_message(app: &Arc<App>, message: &Message) -> bool {
    let Some(guild) = message.guild_id else {
        return false;
    };
    if message.author.bot
        || message
            .author
            .system
            .unwrap_or_default()
        || !matches!(
            message.kind,
            MessageType::Regular | MessageType::Reply
        )
    {
        return false;
    }
    let config = app.guild(guild);
    let quarantine_channel = config.channel(ChannelKind::Quarantine);

    // Before the config check, so an open case stays enforced when its setting is unset.
    let user = message.author.id;
    let record = match app
        .db
        .call(move |c| db::quarantine::get(c, guild, user))
        .await
    {
        Ok(record) => record,
        Err(error) => {
            tracing::error!(%error, "quarantine lookup failed");
            None
        }
    };
    if let Some(record) = record {
        // Staff cases may talk to staff here; automatic cases may be hijacked accounts.
        if record.reason == Reason::Manual && Some(message.channel_id) == quarantine_channel {
            return true;
        }
        let (app2, channel, id) = (Arc::clone(app), message.channel_id, message.id);
        let _ = app.spawn_cleanup("quarantined_message", async move {
            if let Err(error) = app2
                .http
                .delete_message(channel, id)
                .await
                && !errors::is(&error, errors::UNKNOWN_MESSAGE)
            {
                tracing::warn!(%error, "failed to delete message from quarantined member");
            }
            quarantine::remind(&app2, guild, user, &record).await;
        });
        return true;
    }
    if config
        .role_id(RoleKind::Quarantine)
        .is_none()
        || quarantine_channel.is_none()
    {
        return false;
    }

    let observed = Observed {
        guild,
        user,
        channel: message.channel_id,
        message: message.id,
        content: crate::discord::shown_content(message),
        attachments: crate::discord::shown_attachments(message),
    };
    let roles = message
        .member
        .as_ref()
        .map_or(&[][..], |m| m.roles.as_slice());
    if !spam_exempt(app, guild, user, roles)
        && let Verdict::Spam { kind, messages } = app
            .moderation
            .spam
            .observe(&observed, Instant::now())
    {
        metrics::counter!("spam_detected_total").increment(1);
        let originals: Vec<_> = messages
            .iter()
            .map(|(_, m)| *m)
            .collect();
        relay::delete_copies(app, &originals, true);
        quarantine::delete_messages(app, messages.clone());
        let evidence = Evidence {
            summary: match kind {
                spam::SpamKind::Identical => format!(
                    "ส่งข้อความซ้ำใน {} ช่องภายใน 10 นาที",
                    distinct_channels(&messages)
                ),
                spam::SpamKind::Campaign => format!(
                    "ส่งข้อความรูปแบบเดียวกันใน {} ช่องภายใน 10 นาที",
                    distinct_channels(&messages)
                ),
            },
            excerpt: excerpt(message),
            channel: Some(message.channel_id),
        };
        match quarantine::quarantine(
            app,
            guild,
            &message.author,
            Reason::Spam,
            evidence,
            None,
        )
        .await
        {
            Ok(quarantine::QuarantineOutcome::NotConfigured) => {
                tracing::warn!(
                    guild = guild.get(),
                    "spammer not quarantined: quarantine is not set up"
                );
            }
            Ok(_) => {}
            Err(error) => tracing::error!(%error, "failed to quarantine spammer"),
        }
        return true;
    }

    if app
        .moderation
        .scam
        .should_scan(app, guild, message)
    {
        app.moderation.scam.enqueue(
            Arc::clone(app),
            crate::discord::without_reply(message),
        );
        return relay_held(app, guild, message);
    }
    false
}

fn relay_held(
    app: &Arc<App>,
    guild: twilight_model::id::Id<twilight_model::id::marker::GuildMarker>,
    message: &Message,
) -> bool {
    let held = !relay::relay_kinds(app, guild, message.channel_id).is_empty();
    if held {
        crate::ai::spawn_on_message(app, message);
    }
    held
}

/// One announcement in four channels would quarantine an admin. Their images are still scanned.
fn spam_exempt(
    app: &App,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    roles: &[Id<RoleMarker>],
) -> bool {
    holds_any(
        app,
        guild,
        user,
        roles,
        Permissions::ADMINISTRATOR,
    )
}

/// @everyone is left out: powers everyone has don't mark anyone as staff.
fn holds_any(
    app: &App,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    roles: &[Id<RoleMarker>],
    permissions: Permissions,
) -> bool {
    app.cache
        .guild(guild)
        .is_some_and(|g| g.owner_id() == user)
        || roles.iter().any(|role| {
            app.cache
                .role(*role)
                .is_some_and(|r| r.permissions.intersects(permissions))
        })
}

fn distinct_channels(messages: &[spam::MessageRef]) -> usize {
    let mut channels: Vec<_> = messages
        .iter()
        .map(|(c, _)| *c)
        .collect();
    channels.sort_unstable();
    channels.dedup();
    channels.len()
}

pub fn excerpt(message: &Message) -> String {
    let mut text = crate::discord::ui::truncate(crate::discord::shown_content(message).trim(), 400);
    let attachments = crate::discord::shown_attachments(message);
    if !attachments.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        let names: Vec<String> = attachments
            .iter()
            .take(5)
            .map(|a| crate::discord::ui::truncate(&a.filename, 80))
            .collect();
        text.push_str(&format!("ไฟล์แนบ: {}", names.join(", ")));
    }
    text
}

#[cfg(test)]
mod tests {
    use twilight_model::gateway::{event::Event, payload::incoming::RoleCreate};

    use super::*;

    #[tokio::test]
    async fn administrators_are_exempt_from_spam_checks() {
        let app = crate::app::test_app().await;
        let guild = Id::new(77);
        for (id, permissions) in [
            (77u64, Permissions::SEND_MESSAGES),
            (
                501,
                Permissions::MANAGE_MESSAGES | Permissions::KICK_MEMBERS,
            ),
            (502, Permissions::ADMINISTRATOR),
        ] {
            let role = serde_json::from_value(serde_json::json!({
                "id": id.to_string(), "name": format!("r{id}"), "color": 0, "hoist": false,
                "colors": {"primary_color": 0}, "position": 1,
                "permissions": permissions.bits().to_string(), "managed": false,
                "mentionable": false, "flags": 0
            }))
            .unwrap();
            app.cache
                .update(&Event::RoleCreate(RoleCreate {
                    guild_id: guild,
                    role,
                }));
        }
        let user = Id::new(9);
        assert!(!spam_exempt(&app, guild, user, &[]));
        assert!(!spam_exempt(&app, guild, user, &[Id::new(501)]));
        // Manage Messages counts as staff for a scam image (deleted, not quarantined).
        assert!(holds_any(
            &app,
            guild,
            user,
            &[Id::new(501)],
            scam::STAFF
        ));
        assert!(!holds_any(&app, guild, user, &[], scam::STAFF));
        assert!(spam_exempt(
            &app,
            guild,
            user,
            &[Id::new(501), Id::new(502)]
        ));
    }
}
