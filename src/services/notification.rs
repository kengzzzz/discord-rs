use std::{collections::HashMap, time::Duration};

use once_cell::sync::Lazy;
use tokio::{sync::RwLock, task::JoinHandle};

use chrono::{Datelike, Utc};
use twilight_model::id::Id;

use crate::{
    configs::{discord::HTTP, notifications::NOTIFICATIONS},
    dbs::mongo::{
        channel::{Channel, ChannelEnum},
        role::RoleEnum,
    },
    services::{channel::ChannelService, role::RoleService, status::StatusService},
};

pub struct NotificationService;

static HANDLES: Lazy<RwLock<HashMap<u64, Vec<JoinHandle<()>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn next_monday_duration() -> Duration {
    let now = Utc::now();
    let weekday = now.weekday().number_from_monday();
    let days = if weekday == 1 { 7 } else { 8 - weekday } as i64;
    let next_day = now.date_naive() + chrono::Duration::days(days);
    let target = next_day.and_hms_opt(0, 0, 0).expect("valid timestamp");
    let target_dt = chrono::DateTime::<Utc>::from_naive_utc_and_offset(target, Utc);
    let dur = target_dt - now;
    Duration::from_secs(dur.num_seconds() as u64)
}

fn notify_loop(
    channel_id: Id<twilight_model::id::marker::ChannelMarker>,
    role_id: u64,
    message: &str,
    mut calc_delay: impl FnMut() -> Duration + Send + 'static,
) -> JoinHandle<()> {
    let msg = message.to_string();
    tokio::spawn(async move {
        loop {
            let delay = calc_delay();
            tokio::time::sleep(delay).await;
            let _ = HTTP
                .create_message(channel_id)
                .content(&format!("{msg} <@&{role_id}>"))
                .await;
        }
    })
}

fn notify_umbra_loop(
    channel_id: Id<twilight_model::id::marker::ChannelMarker>,
    role_id: u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut last = StatusService::is_umbra_forma();
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let now_state = StatusService::is_umbra_forma();
            if now_state && !last {
                let _ = HTTP
                    .create_message(channel_id)
                    .content(&format!("{} <@&{role_id}>", NOTIFICATIONS.umbra_forma))
                    .await;
            }
            last = now_state;
        }
    })
}

impl NotificationService {
    async fn init_for_channel(ch: Channel) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::new();
        let channel_id = Id::new(ch.channel_id);
        if let Some(role) = RoleService::get_by_type(ch.guild_id, &RoleEnum::Helminth).await {
            handles.push(notify_loop(
                channel_id,
                role.role_id,
                NOTIFICATIONS.helminth,
                next_monday_duration,
            ));
        }
        if let Some(role) = RoleService::get_by_type(ch.guild_id, &RoleEnum::RivenSilver).await {
            handles.push(notify_loop(
                channel_id,
                role.role_id,
                NOTIFICATIONS.riven_sliver,
                next_monday_duration,
            ));
        }
        if let Some(role) = RoleService::get_by_type(ch.guild_id, &RoleEnum::UmbralForma).await {
            handles.push(notify_umbra_loop(channel_id, role.role_id));
        }
        handles
    }

    async fn init_all() -> HashMap<u64, Vec<JoinHandle<()>>> {
        let mut map = HashMap::new();
        let channels = ChannelService::list_by_type(&ChannelEnum::Notification).await;
        for ch in channels {
            map.insert(ch.guild_id, Self::init_for_channel(ch).await);
        }
        map
    }

    async fn init_guild(guild_id: u64) -> Vec<JoinHandle<()>> {
        if let Some(ch) = ChannelService::get_by_type(guild_id, &ChannelEnum::Notification).await {
            Self::init_for_channel(ch).await
        } else {
            Vec::new()
        }
    }

    pub fn spawn() {
        tokio::spawn(async {
            let map = Self::init_all().await;
            *HANDLES.write().await = map;
        });
    }

    pub async fn reload() {
        let map = Self::init_all().await;
        let mut guard = HANDLES.write().await;
        for (_, hs) in guard.drain() {
            for h in hs {
                h.abort();
            }
        }
        *guard = map;
    }

    pub async fn reload_guild(guild_id: u64) {
        let new_handles = Self::init_guild(guild_id).await;
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
