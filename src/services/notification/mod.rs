use std::{collections::HashMap, time::Duration};

use once_cell::sync::Lazy;
use tokio::{sync::RwLock, task::JoinHandle};

use chrono::{DateTime, Datelike, Utc};
use tokio_util::sync::CancellationToken;
use twilight_http::Client;
use twilight_model::id::{Id, marker::ChannelMarker};

use crate::{
    configs::notifications::NOTIFICATIONS,
    context::Context,
    dbs::mongo::{
        channel::{Channel, ChannelEnum},
        role::RoleEnum,
    },
    services::{channel::ChannelService, role::RoleService, shutdown, status::StatusService},
};
use std::sync::Arc;

pub struct NotificationService;

static HANDLES: Lazy<RwLock<HashMap<u64, Vec<JoinHandle<()>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub(crate) fn next_monday_duration() -> Duration {
    let now = Utc::now();
    let weekday = now.weekday().number_from_monday();
    let days = if weekday == 1 { 7 } else { 8 - weekday } as i64;
    let next_day = now.date_naive() + chrono::Duration::days(days);
    let target = next_day.and_hms_opt(0, 0, 0).expect("valid timestamp");
    let target_dt = DateTime::<Utc>::from_naive_utc_and_offset(target, Utc);
    let dur = target_dt - now;
    Duration::from_secs(dur.num_seconds() as u64)
}

pub(crate) fn notify_loop(
    http: Arc<Client>,
    channel_id: Id<ChannelMarker>,
    role_id: u64,
    message: &str,
    mut calc_delay: impl FnMut() -> Duration + Send + 'static,
    token: CancellationToken,
) -> JoinHandle<()> {
    let msg = message.to_string();
    tokio::spawn(async move {
        loop {
            let delay = calc_delay();
            tokio::select! {
                _ = token.cancelled() => break,
                _ = tokio::time::sleep(delay) => {
                    if let Err(e) = http
                        .create_message(channel_id)
                        .content(&format!("{msg} <@&{role_id}>"))
                        .await
                    {
                        tracing::warn!(
                            channel_id = channel_id.get(),
                            role_id,
                            error = %e,
                            "failed to send notification"
                        );
                    }
                }
            }
        }
    })
}

pub(crate) fn notify_umbra_loop(
    http: Arc<Client>,
    channel_id: Id<ChannelMarker>,
    role_id: u64,
    token: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut last = StatusService::is_umbra_forma();
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    let now_state = StatusService::is_umbra_forma();
                    if now_state && !last {
                        if let Err(e) = http
                            .create_message(channel_id)
                            .content(&format!("{} <@&{role_id}>", NOTIFICATIONS.umbra_forma))
                            .await
                        {
                            tracing::warn!(
                                channel_id = channel_id.get(),
                                role_id,
                                error = %e,
                                "failed to send umbra forma notification"
                            );
                        }
                    }
                    last = now_state;
                }
            }
        }
    })
}

impl NotificationService {
    async fn init_for_channel(
        ctx: Arc<Context>,
        ch: Channel,
        token: CancellationToken,
    ) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::new();
        let channel_id = Id::new(ch.channel_id);
        if let Some(role) =
            RoleService::get_by_type(ctx.clone(), ch.guild_id, &RoleEnum::Helminth).await
        {
            handles.push(notify_loop(
                ctx.http.clone(),
                channel_id,
                role.role_id,
                NOTIFICATIONS.helminth,
                next_monday_duration,
                token.clone(),
            ));
        }
        if let Some(role) =
            RoleService::get_by_type(ctx.clone(), ch.guild_id, &RoleEnum::RivenSilver).await
        {
            handles.push(notify_loop(
                ctx.http.clone(),
                channel_id,
                role.role_id,
                NOTIFICATIONS.riven_sliver,
                next_monday_duration,
                token.clone(),
            ));
        }
        if let Some(role) =
            RoleService::get_by_type(ctx.clone(), ch.guild_id, &RoleEnum::UmbralForma).await
        {
            handles.push(notify_umbra_loop(
                ctx.http.clone(),
                channel_id,
                role.role_id,
                token.clone(),
            ));
        }
        handles
    }

    async fn init_all(
        ctx: Arc<Context>,
        token: CancellationToken,
    ) -> HashMap<u64, Vec<JoinHandle<()>>> {
        let mut map = HashMap::new();
        let channels = ChannelService::list_by_type(ctx.clone(), &ChannelEnum::Notification).await;
        for ch in channels {
            map.insert(
                ch.guild_id,
                Self::init_for_channel(ctx.clone(), ch, token.clone()).await,
            );
        }
        map
    }

    async fn init_guild(
        ctx: Arc<Context>,
        guild_id: u64,
        token: CancellationToken,
    ) -> Vec<JoinHandle<()>> {
        if let Some(ch) =
            ChannelService::get_by_type(ctx.clone(), guild_id, &ChannelEnum::Notification).await
        {
            Self::init_for_channel(ctx, ch, token).await
        } else {
            Vec::new()
        }
    }

    pub fn spawn(ctx: Arc<Context>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let token = shutdown::get_token();
            let map = Self::init_all(ctx.clone(), token.clone()).await;
            *HANDLES.write().await = map;

            token.cancelled().await;

            let mut guard = HANDLES.write().await;
            for (_, hs) in guard.drain() {
                for h in hs {
                    h.abort();
                }
            }
        })
    }

    pub async fn reload(ctx: Arc<Context>) {
        let token = shutdown::get_token();
        let map = Self::init_all(ctx, token.clone()).await;
        let mut guard = HANDLES.write().await;
        for (_, hs) in guard.drain() {
            for h in hs {
                h.abort();
            }
        }
        *guard = map;
    }

    pub async fn reload_guild(ctx: Arc<Context>, guild_id: u64) {
        let token = shutdown::get_token();
        let new_handles = Self::init_guild(ctx, guild_id, token.clone()).await;
        let mut guard = HANDLES.write().await;
        if let Some(old) = guard.remove(&guild_id) {
            for h in old {
                h.abort();
            }
        }
        if !new_handles.is_empty() {
            guard.insert(guild_id, new_handles);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
