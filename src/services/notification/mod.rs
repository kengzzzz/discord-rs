pub mod worker;

use std::collections::HashMap;

use once_cell::sync::Lazy;
use tokio::{sync::RwLock, task::JoinHandle};

use tokio_util::sync::CancellationToken;
use twilight_model::id::Id;

use crate::{
    configs::notifications::NOTIFICATIONS,
    context::Context,
    dbs::mongo::models::{
        channel::{Channel, ChannelEnum},
        role::RoleEnum,
    },
    services::{channel::ChannelService, role::RoleService, shutdown, status::StatusService},
};
use std::sync::Arc;

use worker::{next_monday_duration, notify_loop, notify_umbra_loop};

pub struct NotificationService;

static HANDLES: Lazy<RwLock<HashMap<u64, Vec<JoinHandle<()>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

impl NotificationService {
    async fn init_for_channel(
        ctx: Arc<Context>,
        ch: Channel,
        token: CancellationToken,
    ) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::with_capacity(3);
        let channel_id = Id::new(ch.channel_id);
        if let Some(role) =
            RoleService::get_by_type(ctx.clone(), ch.guild_id, &RoleEnum::Helminth).await
        {
            handles.push(notify_loop(
                ctx.clone(),
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
                ctx.clone(),
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
                ctx,
                channel_id,
                role.role_id,
                StatusService::subscribe_umbra_forma(),
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
            let map = Self::init_all(ctx, token.clone()).await;
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
