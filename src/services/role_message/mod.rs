pub mod handler;
pub mod storage;

use std::sync::Arc;

use twilight_model::id::{Id, marker::GuildMarker};

use crate::{context::Context, dbs::mongo::models::message::Message};

pub struct RoleMessageService;

impl RoleMessageService {
    pub async fn get(ctx: Arc<Context>, guild_id: u64) -> Option<Message> {
        storage::get(ctx, guild_id).await
    }

    pub async fn set(ctx: Arc<Context>, guild_id: u64, channel_id: u64, message_id: u64) {
        storage::set(ctx, guild_id, channel_id, message_id).await;
    }

    pub async fn purge_cache(guild_id: u64) {
        storage::purge_cache(guild_id).await;
    }

    pub async fn ensure_message(ctx: Arc<Context>, guild_id: Id<GuildMarker>) {
        handler::ensure_message(ctx, guild_id).await;
    }
}
