use deadpool_redis::Pool;
use mongodb::bson::{doc, to_bson};
use std::sync::Arc;

use crate::{
    configs::CACHE_PREFIX,
    context::Context,
    dbs::{
        mongo::models::message::{Message, MessageEnum},
        redis::{redis_delete, redis_get, redis_set},
    },
};

pub async fn get(ctx: Arc<Context>, guild_id: u64) -> Option<Message> {
    let redis_key = format!("{CACHE_PREFIX}:role-message:{guild_id}");

    if let Some(msg) = redis_get(&ctx.redis, &redis_key).await {
        return Some(msg);
    }

    if let Ok(Some(msg)) = ctx
        .mongo
        .messages
        .find_one(
            doc! {"guild_id": guild_id as i64, "message_type": to_bson(&MessageEnum::Role).ok()},
        )
        .await
    {
        redis_set(&ctx.redis, &redis_key, &msg).await;
        return Some(msg);
    }

    None
}

pub async fn set(ctx: Arc<Context>, guild_id: u64, channel_id: u64, message_id: u64) {
    if let Err(e) = ctx
        .mongo
        .messages
        .update_one(
            doc! {"guild_id": guild_id as i64, "message_type": to_bson(&MessageEnum::Role).ok()},
            doc! {"$set": {"guild_id": guild_id as i64, "channel_id": channel_id as i64, "message_id": message_id as i64, "message_type": to_bson(&MessageEnum::Role).ok()}},
        )
        .upsert(true)
        .await
    {
        tracing::warn!(guild_id, channel_id, message_id, error = %e, "failed to persist role message location");
    }
}

pub async fn purge_cache(pool: &Pool, guild_id: u64) {
    let redis_key = format!("{CACHE_PREFIX}:role-message:{guild_id}");
    redis_delete(pool, &redis_key).await;
}
