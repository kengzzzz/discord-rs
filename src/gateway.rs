use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use twilight_gateway::{
    CloseFrame, ConfigBuilder, Event, EventTypeFlags, Shard, ShardState, StreamExt as _,
    error::ReceiveMessageErrorType,
};
use twilight_model::{
    gateway::payload::incoming::GuildCreate,
    id::{Id, marker::ChannelMarker},
};

use crate::{
    app::App,
    features::{onboarding, relay, roles, voice_log},
    guild::{ChannelKind, RoleKind},
    interactions, moderation, warframe,
};

const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);
/// Twilight's backoff grows to 255 s and outlasts the resume window. Past this many failures the
/// shard is rebuilt with the same session to reset it.
const BACKOFF_RESET_ATTEMPTS: u8 = 5;
/// Handlers stall in a REST outage while events keep coming, so past this they are dropped.
/// Counted in bytes because senders control message size.
pub const EVENT_BACKLOG_KIB: u32 = 4096;
const MIN_EVENT_KIB: u32 = 3;
/// A waiting task plus tokio's header, rounded up. A test checks it.
const TASK_BYTES: usize = 2048;

fn staff(interaction: &twilight_model::application::interaction::Interaction) -> bool {
    use twilight_model::guild::Permissions;
    interaction
        .member
        .as_ref()
        .and_then(|member| member.permissions)
        .is_some_and(|permissions| {
            permissions.intersects(
                Permissions::ADMINISTRATOR
                    | Permissions::MANAGE_GUILD
                    | Permissions::MODERATE_MEMBERS
                    | Permissions::KICK_MEMBERS,
            )
        })
}

pub fn backlog_kib(message: &twilight_model::channel::Message) -> u32 {
    let bytes = crate::discord::approx_size(message) + TASK_BYTES;
    u32::try_from(bytes.div_ceil(1024))
        .unwrap_or(u32::MAX)
        .clamp(MIN_EVENT_KIB, EVENT_BACKLOG_KIB)
}

pub async fn run(app: Arc<App>, mut shard: Shard) -> anyhow::Result<()> {
    let mut outage: Option<Instant> = None;
    let mut warned = false;
    let mut last_close = None;
    let mut unparsed_seen = std::collections::HashSet::new();
    let mut dropping = false;
    let mut dropping_interactions = false;
    loop {
        let item = tokio::select! {
            () = app.shutdown.cancelled() => {
                close(&app, &mut shard).await;
                return Ok(());
            }
            item = shard.next_event(EventTypeFlags::all()) => item,
        };
        let Some(item) = item else {
            app.health.set_gateway(false, None);
            // Exiting would restart into the same refusal and spend an IDENTIFY each time.
            tracing::error!(code = ?last_close, "gateway closed with a non-resumable code; fix the cause and restart the bot");
            app.shutdown.cancelled().await;
            return Ok(());
        };
        let mut event = match item {
            Ok(event) => event,
            Err(error) => {
                // The error text holds the raw event, message content included.
                if let ReceiveMessageErrorType::Deserializing { event } = error.kind() {
                    unparsed(&mut unparsed_seen, event);
                    continue;
                }
                let identified = shard.state().is_identified();
                app.health.set_gateway(identified, None);
                if identified {
                    tracing::debug!(?error, "gateway receive error");
                    continue;
                }
                outage.get_or_insert_with(Instant::now);
                if warned {
                    tracing::debug!(?error, "gateway still unreachable");
                } else {
                    warned = true;
                    tracing::warn!(?error, "gateway unreachable; reconnecting");
                }
                if matches!(error.kind(), ReceiveMessageErrorType::Reconnect)
                    && let ShardState::Disconnected { reconnect_attempts } = shard.state()
                    && reconnect_attempts >= BACKOFF_RESET_ATTEMPTS
                {
                    tracing::debug!(
                        reconnect_attempts,
                        "resetting gateway reconnect backoff"
                    );
                    shard = with_fresh_backoff(&shard);
                }
                continue;
            }
        };
        match &event {
            Event::GatewayClose(frame) => {
                last_close = frame.as_ref().map(|f| f.code);
                if outage.is_none() {
                    outage = Some(Instant::now());
                    tracing::info!(code = ?last_close, "gateway disconnected; reconnecting");
                }
            }
            Event::Ready(_) | Event::Resumed => {
                if let Some(since) = outage.take() {
                    warned = false;
                    tracing::info!(
                        resumed = matches!(event, Event::Resumed),
                        offline_secs = since.elapsed().as_secs(),
                        "gateway reconnected"
                    );
                }
            }
            _ => {}
        }

        let previous_voice = update_cache(&app, &mut event);
        match &mut event {
            Event::MessageCreate(message) => crate::discord::slim(&mut message.message, app.bot_id),
            Event::InteractionCreate(interaction) => crate::discord::slim_interaction(interaction),
            _ => {}
        }
        app.health.set_gateway(
            shard.state().is_identified(),
            shard
                .latency()
                .average()
                .map(|d| d.as_millis() as u64),
        );
        metrics::counter!("gateway_events_total", "kind" => event_name(&event)).increment(1);

        if let Event::MemberAdd(member) = &event {
            let user = &member.member.user;
            if !user.bot && !user.system.unwrap_or_default() {
                onboarding::queue_join(&app, member.guild_id, user.id);
            }
            continue;
        }
        // Here, not behind the backlog, so a queued rejoin never sees the member as present.
        if let Event::MemberRemove(member) = &event {
            onboarding::on_leave(member.guild_id, member.user.id);
            let channel = voices(&app).remove(member.guild_id, member.user.id);
            if let Some(channel) = channel {
                voice_log::on_member_leave(&app, member.guild_id, &member.user, channel);
            }
            continue;
        }
        if !handled(&event) {
            continue;
        }
        // Interactions, Ready and GuildCreate never wait in the backlog.
        let essential = matches!(event, Event::Ready(_) | Event::GuildCreate(_));
        let mut interaction = None;
        let slot = if essential {
            None
        } else if let Event::InteractionCreate(created) = &event {
            match app
                .interactions
                .start(created.author_id(), staff(created))
            {
                Ok(slot) => {
                    interaction = Some(slot);
                    if dropping_interactions {
                        dropping_interactions = false;
                        tracing::info!("interactions caught up");
                    }
                    None
                }
                Err(refused) => {
                    let full = matches!(refused, crate::app::InteractionRefused::Full);
                    metrics::counter!("interactions_dropped_total", "reason" => if full { "full" } else { "member" }).increment(1);
                    if full && !dropping_interactions {
                        dropping_interactions = true;
                        tracing::warn!("too many interactions at once; dropping them");
                    }
                    continue;
                }
            }
        } else {
            let kib = match &event {
                Event::MessageCreate(message) => backlog_kib(&message.message),
                _ => MIN_EVENT_KIB,
            };
            match Arc::clone(&app.event_backlog).try_acquire_many_owned(kib) {
                Ok(slot) => Some(slot),
                Err(_) => {
                    metrics::counter!("gateway_events_dropped_total").increment(1);
                    if !dropping {
                        dropping = true;
                        tracing::warn!(
                            backlog_kib = EVENT_BACKLOG_KIB,
                            "event handlers are stalled; dropping events"
                        );
                    }
                    continue;
                }
            }
        };
        if dropping && slot.is_some() {
            dropping = false;
            tracing::info!("event handlers caught up");
        }
        app.spawn(waiting(
            Arc::clone(&app),
            event,
            previous_voice,
            (slot, interaction),
        ));
    }
}

/// Only what guild, channel and role lookups and the bot's own member need reaches the cache, so
/// it can't grow per member. Returns a VoiceStateUpdate's previous channel.
fn update_cache(app: &App, event: &mut Event) -> Option<Id<ChannelMarker>> {
    match event {
        Event::GuildCreate(guild) => {
            if let GuildCreate::Available(guild) = &mut **guild {
                guild
                    .members
                    .retain(|m| m.user.id == app.bot_id);
                guild.threads.clear();
                let states = std::mem::take(&mut guild.voice_states);
                voices(app).reset(guild.id, &states);
            }
            app.cache.update(&**guild);
        }
        Event::GuildDelete(deleted) => {
            voices(app).reset(deleted.id, &[]);
            app.cache.update(deleted);
        }
        Event::UnavailableGuild(unavailable) => {
            voices(app).reset(unavailable.id, &[]);
            app.cache.update(&*unavailable);
        }
        Event::MemberAdd(added) if added.member.user.id == app.bot_id => app.cache.update(&**added),
        Event::MemberUpdate(updated) if updated.user.id == app.bot_id => {
            app.cache.update(&**updated);
        }
        Event::MemberRemove(removed) if removed.user.id == app.bot_id => {
            app.cache.update(&*removed)
        }
        Event::VoiceStateUpdate(update) => {
            let guild = update.guild_id?;
            return voices(app).update(guild, update.user_id, update.channel_id);
        }
        Event::Ready(ready) => {
            let guilds: Vec<_> = ready
                .guilds
                .iter()
                .map(|g| g.id)
                .collect();
            voices(app).keep_guilds(&guilds);
            app.cache.update(&*ready);
        }
        Event::GuildUpdate(_)
        | Event::ChannelCreate(_)
        | Event::ChannelUpdate(_)
        | Event::ChannelDelete(_)
        | Event::RoleCreate(_)
        | Event::RoleUpdate(_)
        | Event::RoleDelete(_)
        | Event::UserUpdate(_) => app.cache.update(&*event),
        _ => {}
    }
    None
}

fn voices(app: &App) -> std::sync::MutexGuard<'_, crate::features::voice_log::Voices> {
    app.voices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Logs each unparsed type once, never the raw event (it can hold message content).
fn unparsed(seen: &mut std::collections::HashSet<String>, event: &str) {
    #[derive(serde::Deserialize)]
    struct Kind {
        t: Option<String>,
    }
    let kind = serde_json::from_str::<Kind>(event)
        .ok()
        .and_then(|k| k.t)
        .unwrap_or_else(|| "unknown".to_owned());
    metrics::counter!("gateway_parse_errors_total", "t" => kind.clone()).increment(1);
    if seen.insert(kind.clone()) {
        tracing::warn!(
            t = kind,
            "gateway event could not be parsed and was skipped; further ones of this type are counted, not logged"
        );
    }
}

/// Only call after a failed connect, when there is no connection to abandon.
fn with_fresh_backoff(shard: &Shard) -> Shard {
    let mut config = ConfigBuilder::from(shard.config().clone());
    if let Some(session) = shard.session() {
        config = config.session(session.clone());
        if let Some(url) = shard.resume_url() {
            config = config.resume_url(url.to_owned());
        }
    }
    Shard::with_config(shard.id(), config.build())
}

/// Twilight writes the close frame only while polled, and polling past `GatewayClose` reconnects.
async fn close(app: &App, shard: &mut Shard) {
    app.health.set_gateway(false, None);
    if !shard.state().is_identified() {
        return;
    }
    shard.close(CloseFrame::NORMAL);
    let acknowledged = tokio::time::timeout(CLOSE_TIMEOUT, async {
        while let Some(item) = shard
            .next_event(EventTypeFlags::all())
            .await
        {
            if matches!(item, Ok(Event::GatewayClose(_))) {
                return;
            }
        }
    })
    .await;
    if acknowledged.is_err() {
        tracing::warn!("gateway close was not acknowledged in time");
    } else {
        tracing::info!("gateway closed");
    }
}

/// Kept small; handler state is allocated once a slot is free.
type Held = (
    Option<tokio::sync::OwnedSemaphorePermit>,
    Option<crate::app::InteractionSlot>,
);

async fn waiting(
    app: Arc<App>,
    event: Event,
    previous_voice: Option<Id<ChannelMarker>>,
    held: Held,
) {
    let _held = held;
    dispatch(app, event, previous_voice).await;
}

async fn dispatch(app: Arc<App>, event: Event, previous_voice: Option<Id<ChannelMarker>>) {
    if let Event::InteractionCreate(interaction) = event {
        Box::pin(interactions::handle(app, interaction.0)).await;
        return;
    }
    let Ok(_permit) = app.event_permits.acquire().await else {
        return;
    };
    Box::pin(handle(Arc::clone(&app), event, previous_voice)).await;
}

async fn handle(app: Arc<App>, event: Event, previous_voice: Option<Id<ChannelMarker>>) {
    match event {
        Event::Ready(ready) => {
            app.health.set_ready(true);
            tracing::info!(user = %ready.user.name, guilds = ready.guilds.len(), "connected to gateway");
            if app.started.set(()).is_ok() {
                warframe::start(&app);
                moderation::start(&app);
                crate::ai::start(&app);
                roles::start(&app);
            }
        }
        // An unavailable guild's roles were just dropped from the cache.
        Event::GuildCreate(guild) if matches!(*guild, GuildCreate::Available(_)) => {
            let guild_id = guild.id();
            roles::ensure_picker(&app, guild_id).await;
            moderation::quarantine::reconcile_guild(&app, guild_id).await;
        }
        Event::MessageCreate(message) => {
            let message = message.message;
            if moderation::on_message(&app, &message).await {
                return;
            }
            // Relay and AI can wait minutes, so they don't hold the event slot.
            if let Some(guild) = message.guild_id
                && !relay::relay_kinds(&app, guild, message.channel_id).is_empty()
            {
                let (app2, message) = (
                    Arc::clone(&app),
                    crate::discord::without_reply(&message),
                );
                app.spawn_relay(async move {
                    relay::on_message(&app2, message).await;
                });
            }
            if message
                .mentions
                .iter()
                .any(|m| m.id == app.bot_id)
            {
                crate::ai::spawn_on_message(&app, &message);
            }
        }
        Event::MessageDelete(deleted) => {
            if let Some(guild) = deleted.guild_id {
                onboarding::on_deleted(guild, &[deleted.id]);
                roles::on_deleted(&app, guild, &[deleted.id]).await;
                warframe::status::on_deleted(&app, guild, &[deleted.id]).await;
            }
            relay::delete_copies(&app, &[deleted.id], false);
        }
        Event::MessageDeleteBulk(deleted) => {
            if let Some(guild) = deleted.guild_id {
                onboarding::on_deleted(guild, &deleted.ids);
                roles::on_deleted(&app, guild, &deleted.ids).await;
                warframe::status::on_deleted(&app, guild, &deleted.ids).await;
            }
            relay::delete_copies(&app, &deleted.ids, false);
        }
        Event::VoiceStateUpdate(update) => {
            voice_log::on_update(&app, &update.0, previous_voice).await
        }
        Event::RoleDelete(role) => match app
            .guilds
            .clear_role(role.guild_id, role.role_id)
            .await
        {
            Ok(cleared) => {
                if cleared.contains(&RoleKind::Quarantine) {
                    tracing::warn!(
                        guild = role.guild_id.get(),
                        "quarantine role deleted; spam and scam detection are off until a new one is set"
                    );
                }
                if cleared
                    .iter()
                    .any(|kind| RoleKind::PICKABLE.contains(kind))
                {
                    roles::ensure_picker(&app, role.guild_id).await;
                }
            }
            Err(error) => tracing::warn!(%error, "failed to drop deleted role from config"),
        },
        // A renamed picker role keeps its old name on the picker otherwise.
        Event::RoleUpdate(update) => {
            // Staff powers count beyond @everyone's, so its change can hide or show a role.
            if update.role.id.cast() == update.guild_id
                || app
                    .guild(update.guild_id)
                    .pickable_role(update.role.id)
                    .is_some()
            {
                roles::ensure_picker(&app, update.guild_id).await;
            }
        }
        Event::ChannelDelete(channel) => {
            if let Some(guild) = channel.guild_id {
                match app
                    .guilds
                    .clear_channel(guild, channel.id)
                    .await
                {
                    Ok(cleared) if cleared.contains(&ChannelKind::Quarantine) => {
                        tracing::warn!(
                            guild = guild.get(),
                            "quarantine channel deleted; spam and scam detection are off until a new one is set"
                        );
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, "failed to drop deleted channel from config")
                    }
                }
            }
        }
        _ => {}
    }
}

fn handled(event: &Event) -> bool {
    matches!(
        event,
        Event::InteractionCreate(_)
            | Event::Ready(_)
            | Event::GuildCreate(_)
            | Event::MessageCreate(_)
            | Event::MessageDelete(_)
            | Event::MessageDeleteBulk(_)
            | Event::VoiceStateUpdate(_)
            | Event::RoleDelete(_)
            | Event::RoleUpdate(_)
            | Event::ChannelDelete(_)
    )
}

fn event_name(event: &Event) -> &'static str {
    match event {
        Event::InteractionCreate(_) => "interaction",
        Event::MessageCreate(_) => "message_create",
        Event::MessageDelete(_) | Event::MessageDeleteBulk(_) => "message_delete",
        Event::MemberAdd(_) => "member_add",
        Event::MemberRemove(_) => "member_remove",
        Event::VoiceStateUpdate(_) => "voice_state",
        Event::GuildCreate(_) => "guild_create",
        Event::Ready(_) => "ready",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn returned2<A, B, R>(_: fn(A, B) -> R) -> usize {
        std::mem::size_of::<R>()
    }

    /// Each interaction holds this boxed handler plus its slimmed payload.
    #[test]
    fn an_interaction_handler_stays_small() {
        let handler = returned2(interactions::handle as fn(_, _) -> _);
        assert!(
            handler <= 32 * 1024,
            "interaction handler {handler} B"
        );
    }

    #[test]
    fn a_waiting_event_fits_its_charge() {
        fn returned4<A, B, C, D, R>(_: fn(A, B, C, D) -> R) -> usize {
            std::mem::size_of::<R>()
        }
        let task = returned4(waiting as fn(_, _, _, _) -> _);
        // Tokio adds a task header and the tracker its token (about 230 B measured).
        assert!(
            task + 512 <= TASK_BYTES,
            "waiting future {task} B"
        );
    }

    #[tokio::test]
    async fn only_the_bots_own_member_is_cached() {
        let app = crate::app::test_app().await;
        let join = |user: u64| -> Event {
            let json = format!(
                r#"{{"t":"GUILD_MEMBER_ADD","s":1,"op":0,"d":{{"guild_id":"10","roles":[],"joined_at":"2026-11-10T00:00:00.000000+00:00","deaf":false,"mute":false,"flags":0,"user":{{"id":"{user}","username":"u","discriminator":"0","avatar":null}}}}}}"#
            );
            twilight_gateway::parse(json, EventTypeFlags::all())
                .unwrap()
                .unwrap()
                .into()
        };
        let parse = |json: &str| -> Event {
            twilight_gateway::parse(json.to_owned(), EventTypeFlags::all())
                .unwrap()
                .unwrap()
                .into()
        };
        let guild = Id::new(10);
        let bot = app.bot_id.get();
        let member = |user: u64| {
            format!(
                r#"{{"roles":[],"joined_at":"2026-11-10T00:00:00.000000+00:00","deaf":false,"mute":false,"flags":0,"user":{{"id":"{user}","username":"u","discriminator":"0","avatar":null}}}}"#
            )
        };
        let voice = |user: &str, channel: &str, extra: &str| {
            format!(
                r#"{{"user_id":"{user}","channel_id":{channel},"session_id":"s","deaf":false,"mute":false,"self_deaf":false,"self_mute":false,"self_video":false,"suppress":false,"request_to_speak_timestamp":null{extra}}}"#
            )
        };
        update_cache(
            &app,
            &mut parse(&format!(
                r#"{{"t":"GUILD_CREATE","s":1,"op":0,"d":{{"id":"10","name":"g","owner_id":"1","afk_timeout":60,"default_message_notifications":0,"explicit_content_filter":0,"features":[],"mfa_level":0,"preferred_locale":"en-US","roles":[],"system_channel_flags":0,"premium_progress_bar_enabled":false,"verification_level":0,"nsfw_level":0,"channels":[],"threads":[{{"id":"41","type":11,"guild_id":"10","parent_id":"20","name":"t"}}],"members":[{},{}],"member_count":100,"voice_states":[{}]}}}}"#,
                member(bot),
                member(7),
                voice("8", r#""50""#, ""),
            )),
        );
        assert!(
            app.cache
                .member(guild, app.bot_id)
                .is_some()
        );
        assert!(
            app.cache
                .member(guild, Id::new(7))
                .is_none()
        );
        assert!(app.cache.channel(Id::new(41)).is_none());
        assert_eq!(app.voices.lock().unwrap().tracked(), 1);
        let voice_update = |channel: &str| {
            parse(&format!(
                r#"{{"t":"VOICE_STATE_UPDATE","s":4,"op":0,"d":{}}}"#,
                voice(
                    "8",
                    channel,
                    &format!(r#","guild_id":"10","member":{}"#, member(8))
                )
            ))
        };
        assert_eq!(
            update_cache(&app, &mut voice_update(r#""51""#)),
            Some(Id::new(50))
        );
        assert_eq!(app.cache.stats().voice_states(), 0);
        assert!(
            app.cache
                .member(guild, Id::new(8))
                .is_none()
        );
        assert_eq!(
            update_cache(&app, &mut voice_update("null")),
            Some(Id::new(51))
        );
        assert_eq!(app.voices.lock().unwrap().tracked(), 0);
        update_cache(
            &app,
            &mut parse(
                r#"{"t":"GUILD_MEMBER_REMOVE","s":5,"op":0,"d":{"guild_id":"10","user":{"id":"7","username":"u","discriminator":"0","avatar":null}}}"#,
            ),
        );
        assert_eq!(
            app.cache
                .guild(guild)
                .unwrap()
                .member_count(),
            Some(100)
        );
        update_cache(&app, &mut join(5));
        update_cache(&app, &mut join(bot));
        update_cache(
            &app,
            &mut parse(
                r#"{"t":"MESSAGE_UPDATE","s":2,"op":0,"d":{"id":"30","channel_id":"20","guild_id":"10","author":{"id":"6","username":"u","discriminator":"0","avatar":null},"member":{"roles":[],"joined_at":"2026-11-10T00:00:00.000000+00:00","deaf":false,"mute":false,"flags":0},"content":"x","timestamp":"2026-11-10T00:00:00.000000+00:00","edited_timestamp":"2026-11-10T00:00:01.000000+00:00","tts":false,"mention_everyone":false,"mentions":[],"mention_roles":[],"attachments":[],"embeds":[],"pinned":false,"type":0}}"#,
            ),
        );
        update_cache(
            &app,
            &mut parse(
                r#"{"t":"THREAD_CREATE","s":3,"op":0,"d":{"id":"40","type":11,"guild_id":"10","parent_id":"20","name":"t"}}"#,
            ),
        );
        assert!(
            app.cache
                .member(guild, Id::new(6))
                .is_none()
        );
        assert!(app.cache.user(Id::new(6)).is_none());
        assert!(app.cache.channel(Id::new(40)).is_none());
        assert!(
            app.cache
                .member(guild, Id::new(5))
                .is_none()
        );
        assert!(
            app.cache
                .member(guild, app.bot_id)
                .is_some()
        );
    }
}
