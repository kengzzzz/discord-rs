use futures::StreamExt;
use mongodb::bson::doc;

use crate::{
    configs::CACHE_PREFIX,
    context::Context,
    dbs::{
        mongo::models::channel::{Channel, ChannelEnum},
        redis::{redis_delete, redis_get, redis_set},
    },
};
use std::sync::Arc;

pub struct ChannelService;

impl ChannelService {
    pub async fn get(ctx: Arc<Context>, channel_id: u64) -> Vec<Channel> {
        let redis_key = format!("{CACHE_PREFIX}:channel:{channel_id}");

        if let Some(Some(channels)) = redis_get::<Option<Vec<Channel>>>(&redis_key).await {
            return channels;
        }

        let mut channels = Vec::new();

        if let Ok(mut cursor) = ctx
            .mongo
            .channels
            .find(doc! {
                "channel_id": channel_id as i64
            })
            .await
        {
            while let Some(Ok(channel)) = cursor.next().await {
                channels.push(channel);
            }

            redis_set(&redis_key, &channels).await;
        }

        channels
    }

    pub async fn purge_cache(channel_id: u64) {
        let redis_key = format!("{CACHE_PREFIX}:channel:{channel_id}");
        redis_delete(&redis_key).await;
    }

    pub async fn get_by_type(
        ctx: Arc<Context>,
        guild_id: u64,
        channel_type: &ChannelEnum,
    ) -> Option<Channel> {
        let redis_key = format!(
            "{}:channel-type:{}:{}",
            CACHE_PREFIX,
            guild_id,
            channel_type.value()
        );

        if let Some(channel) = redis_get(&redis_key).await {
            return Some(channel);
        }

        if let Ok(Some(ch)) = ctx
            .mongo
            .channels
            .find_one(doc! {"guild_id": guild_id as i64, "channel_type": channel_type.value()})
            .await
        {
            redis_set(&redis_key, &ch).await;
            return Some(ch);
        }

        None
    }

    pub async fn purge_cache_by_type(guild_id: u64, channel_type: &ChannelEnum) {
        let redis_key = format!(
            "{}:channel-type:{}:{}",
            CACHE_PREFIX,
            guild_id,
            channel_type.value()
        );
        redis_delete(&redis_key).await;
    }

    pub async fn purge_list_cache(channel_type: &ChannelEnum) {
        let redis_key = format!("{}:channels-by-type:{}", CACHE_PREFIX, channel_type.value());
        redis_delete(&redis_key).await;
    }

    pub async fn list_by_type(ctx: Arc<Context>, channel_type: &ChannelEnum) -> Vec<Channel> {
        let redis_key = format!("{}:channels-by-type:{}", CACHE_PREFIX, channel_type.value());

        if let Some(Some(channels)) = redis_get(&redis_key).await {
            return channels;
        }

        let mut channels = Vec::new();
        if let Ok(mut cursor) = ctx
            .mongo
            .channels
            .find(doc! { "channel_type": channel_type.value() })
            .await
        {
            while let Some(Ok(channel)) = cursor.next().await {
                channels.push(channel);
            }

            redis_set(&redis_key, &channels).await;
        }

        channels
    }
}
