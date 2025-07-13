use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::{channel::Message, id::Id};

use crate::{
    configs::{CACHE_PREFIX, Reaction},
    context::Context,
    dbs::{
        mongo::channel::ChannelEnum,
        redis::{redis_delete, redis_get, redis_set_ex},
    },
    services::channel::ChannelService,
    utils::embed,
};
use std::sync::Arc;

pub struct BroadcastService;

const TTL: usize = 600;

impl BroadcastService {
    pub async fn handle(ctx: Arc<Context>, message: &Message) {
        let Some(guild_id) = message.guild_id else {
            return;
        };

        let Some(guild_ref) = ctx.cache.guild(guild_id) else {
            return;
        };

        let Ok(embeds) = embed::broadcast_embeds(&guild_ref, message) else {
            return;
        };

        let mut records = Vec::new();
        for channel in ChannelService::list_by_type(ctx.clone(), &ChannelEnum::Broadcast).await {
            if channel.channel_id == message.channel_id.get() {
                continue;
            }
            let channel_id = Id::new(channel.channel_id);
            if let Ok(resp) = ctx.http.create_message(channel_id).embeds(&embeds).await {
                if let Ok(msg) = resp.model().await {
                    records.push((channel.channel_id, msg.id.get()));
                }
            }
        }
        if !records.is_empty() {
            Self::remember(message.id.get(), &records).await;
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

    async fn remember(original: u64, records: &Vec<(u64, u64)>) {
        let key = format!("{CACHE_PREFIX}:broadcast:{original}");
        redis_set_ex(&key, records, TTL).await;
    }

    pub async fn delete_replicas(ctx: Arc<Context>, messages: &[(u64, u64)]) {
        for &(_, msg_id) in messages {
            let key = format!("{CACHE_PREFIX}:broadcast:{msg_id}");
            if let Some(list) = redis_get::<Vec<(u64, u64)>>(&key).await {
                for (ch, m) in list {
                    if let Err(e) = ctx.http.delete_message(Id::new(ch), Id::new(m)).await {
                        tracing::warn!(channel_id = ch, message_id = m, error = %e, "failed to delete broadcast replica");
                    }
                }
                redis_delete(&key).await;
            }
        }
    }
}
