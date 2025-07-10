use crate::{
    configs::CACHE_PREFIX,
    dbs::{
        mongo::{
            message::{Message, MessageEnum},
            mongodb::MongoDB,
        },
        redis::{redis_delete, redis_get, redis_set},
    },
};
use mongodb::bson::{doc, to_bson};

pub struct StatusMessageService;

impl StatusMessageService {
    pub async fn get(guild_id: u64) -> Option<Message> {
        let redis_key = format!("{CACHE_PREFIX}:status-message:{guild_id}");

        if let Some(msg) = redis_get(&redis_key).await {
            return Some(msg);
        }

        if let Ok(Some(msg)) = MongoDB::get()
            .messages
            .find_one(doc! {"guild_id": guild_id as i64, "message_type": to_bson(&MessageEnum::Status).ok()})
            .await
        {
            redis_set(&redis_key, &msg).await;
            return Some(msg);
        }

        None
    }

    pub async fn set(guild_id: u64, channel_id: u64, message_id: u64) {
        let _ = MongoDB::get()
            .messages
            .update_one(
                doc! {"guild_id": guild_id as i64, "message_type": to_bson(&MessageEnum::Status).ok()},
                doc! {"$set": {"guild_id": guild_id as i64, "channel_id": channel_id as i64, "message_id": message_id as i64, "message_type": to_bson(&MessageEnum::Status).ok()}},
            )
            .upsert(true)
            .await;
    }

    pub async fn purge_cache(guild_id: u64) {
        let redis_key = format!("{CACHE_PREFIX}:status-message:{guild_id}");
        redis_delete(&redis_key).await;
    }
}
