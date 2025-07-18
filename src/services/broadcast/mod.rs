pub mod embed;

use deadpool_redis::Pool;
use futures::{StreamExt as _, stream};
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::{channel::Message, id::Id};

use crate::{
    configs::{CACHE_PREFIX, Reaction},
    context::Context,
    dbs::{
        mongo::models::channel::ChannelEnum,
        redis::{redis_delete, redis_get, redis_set_ex},
    },
    services::channel::ChannelService,
};
use std::sync::Arc;

pub struct BroadcastService;

const TTL: usize = 600;
const DISPATCH_CONCURRENCY: usize = 5;

impl BroadcastService {
    pub async fn handle(ctx: Arc<Context>, message: &Message) {
        let Some(guild_id) = message.guild_id else {
            return;
        };

        let Some(guild_ref) = ctx.cache.guild(guild_id) else {
            return;
        };

        let Ok(embeds) = Self::broadcast_embeds(&guild_ref, message) else {
            return;
        };

        let channels = ChannelService::list_by_type(&ctx, &ChannelEnum::Broadcast).await;
        let records: Vec<(u64, u64)> = stream::iter(channels)
            .filter(|ch| futures::future::ready(ch.channel_id != message.channel_id.get()))
            .map(|channel| {
                let ctx = ctx.clone();
                let embeds = embeds.clone();
                async move {
                    let channel_id = Id::new(channel.channel_id);
                    if let Ok(resp) = ctx.http.create_message(channel_id).embeds(&embeds).await {
                        if let Ok(msg) = resp.model().await {
                            return Some((channel.channel_id, msg.id.get()));
                        }
                    }
                    None
                }
            })
            .buffer_unordered(DISPATCH_CONCURRENCY)
            .filter_map(|r| async move { r })
            .collect()
            .await;

        if !records.is_empty() {
            Self::remember(&ctx.redis, message.id.get(), &records).await;
        }

        let emoji = RequestReactionType::Unicode {
            name: Reaction::Success.emoji(),
        };
        if let Err(e) = ctx
            .http
            .create_reaction(message.channel_id, message.id, &emoji)
            .await
        {
            tracing::warn!(channel_id = message.channel_id.get(), message_id = message.id.get(), error = %e, "failed to react to broadcast");
        }
    }

    async fn remember(pool: &Pool, original: u64, records: &Vec<(u64, u64)>) {
        let key = format!("{CACHE_PREFIX}:broadcast:{original}");
        redis_set_ex(pool, &key, records, TTL).await;
    }

    pub async fn delete_replicas(ctx: Arc<Context>, messages: &[(u64, u64)]) {
        for &(_, msg_id) in messages {
            let key = format!("{CACHE_PREFIX}:broadcast:{msg_id}");
            if let Some(list) = redis_get::<Vec<(u64, u64)>>(&ctx.redis, &key).await {
                for (ch, m) in list {
                    if let Err(e) = ctx.http.delete_message(Id::new(ch), Id::new(m)).await {
                        tracing::warn!(channel_id = ch, message_id = m, error = %e, "failed to delete broadcast replica");
                    }
                }
                redis_delete(&ctx.redis, &key).await;
            }
        }
    }
}
