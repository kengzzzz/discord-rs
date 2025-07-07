use crate::{
    configs::CACHE_PREFIX,
    dbs::{
        mongo::{mongodb::MongoDB, status_message::StatusMessage},
        redis::{redis_delete, redis_get, redis_set},
    },
};
use mongodb::bson::doc;

pub struct StatusMessageService;

impl StatusMessageService {
    pub async fn get(guild_id: u64) -> Option<StatusMessage> {
        let redis_key = format!("{CACHE_PREFIX}:status-message:{guild_id}");

        if let Some(msg) = redis_get(&redis_key).await {
            return Some(msg);
        }

        if let Ok(Some(msg)) = MongoDB::get()
            .status_messages
            .find_one(doc! {"guild_id": guild_id as i64})
            .await
        {
            redis_set(&redis_key, &msg).await;
            return Some(msg);
        }

        None
    }

    pub async fn set(guild_id: u64, channel_id: u64, message_id: u64) {
        let _ = MongoDB::get()
            .status_messages
            .update_one(
                doc! {"guild_id": guild_id as i64},
                doc! {"$set": {"guild_id": guild_id as i64, "channel_id": channel_id as i64, "message_id": message_id as i64}},
            )
            .upsert(true)
            .await;
    }

    pub async fn purge_cache(guild_id: u64) {
        let redis_key = format!("{CACHE_PREFIX}:status-message:{guild_id}");
        redis_delete(&redis_key).await;
    }
}
