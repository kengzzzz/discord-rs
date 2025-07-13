use crate::{
    configs::CACHE_PREFIX,
    context::Context,
    dbs::{
        mongo::message::{Message, MessageEnum},
        redis::{redis_delete, redis_get, redis_set},
    },
};
use mongodb::bson::{doc, to_bson};
use std::sync::Arc;

pub struct StatusMessageService;

impl StatusMessageService {
    pub async fn get(ctx: Arc<Context>, guild_id: u64) -> Option<Message> {
        let redis_key = format!("{CACHE_PREFIX}:status-message:{guild_id}");

        if let Some(msg) = redis_get(&redis_key).await {
            return Some(msg);
        }

        if let Ok(Some(msg)) = ctx.mongo
            .messages
            .find_one(doc! {"guild_id": guild_id as i64, "message_type": to_bson(&MessageEnum::Status).ok()})
            .await
        {
            redis_set(&redis_key, &msg).await;
            return Some(msg);
        }

        None
    }

    pub async fn set(ctx: Arc<Context>, guild_id: u64, channel_id: u64, message_id: u64) {
        if let Err(e) = ctx.mongo
            .messages
            .update_one(
                doc! {"guild_id": guild_id as i64, "message_type": to_bson(&MessageEnum::Status).ok()},
                doc! {"$set": {"guild_id": guild_id as i64, "channel_id": channel_id as i64, "message_id": message_id as i64, "message_type": to_bson(&MessageEnum::Status).ok()}},
            )
            .upsert(true)
            .await
        {
            tracing::warn!(guild_id, channel_id, message_id, error = %e, "failed to persist status message location");
        }
    }

    pub async fn purge_cache(guild_id: u64) {
        let redis_key = format!("{CACHE_PREFIX}:status-message:{guild_id}");
        redis_delete(&redis_key).await;
    }
}
