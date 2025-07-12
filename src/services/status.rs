use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::task::JoinHandle;
use twilight_cache_inmemory::Reference;
use twilight_cache_inmemory::model::CachedGuild;
use twilight_model::channel::message::Embed;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::services::shutdown;
use crate::{
    context::Context,
    dbs::mongo::channel::ChannelEnum,
    services::{channel::ChannelService, status_message::StatusMessageService},
    warframe,
};
use std::sync::Arc;

static UMBRA_FORMA: AtomicBool = AtomicBool::new(false);

pub struct StatusService;

impl StatusService {
    pub fn is_umbra_forma() -> bool {
        UMBRA_FORMA.load(Ordering::Relaxed)
    }

    async fn build_embed(
        ctx: Arc<Context>,
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    ) -> Option<Embed> {
        match warframe::status_embed(ctx.clone(), guild).await {
            Ok((e, is_umbra)) => {
                UMBRA_FORMA.store(is_umbra, Ordering::Relaxed);
                Some(e)
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to build status embed");
                None
            }
        }
    }

    pub async fn update_all(ctx: Arc<Context>) {
        for channel in ChannelService::list_by_type(ctx.clone(), &ChannelEnum::Status).await {
            let Some(guild_ref) = ctx.cache.guild(Id::new(channel.guild_id)) else {
                continue;
            };
            let Some(embed) = Self::build_embed(ctx.clone(), &guild_ref).await else {
                continue;
            };
            let channel_id = Id::new(channel.channel_id);
            let mut existing = None;
            if let Some(record) = StatusMessageService::get(ctx.clone(), channel.guild_id).await {
                if ctx
                    .http
                    .message(channel_id, Id::new(record.message_id))
                    .await
                    .is_ok()
                {
                    existing = Some(record.message_id);
                }
            }

            if let Some(msg_id) = existing {
                let _ = ctx
                    .http
                    .update_message(channel_id, Id::new(msg_id))
                    .embeds(Some(&[embed.clone()]))
                    .await;
                StatusMessageService::set(
                    ctx.clone(),
                    channel.guild_id,
                    channel.channel_id,
                    msg_id,
                )
                .await;
            } else {
                if let Ok(resp) = ctx.http.channel_messages(channel_id).await {
                    if let Ok(msgs) = resp.model().await {
                        let ids: Vec<_> = msgs.into_iter().map(|m| m.id).collect();

                        for chunk in ids.chunks(100) {
                            if chunk.len() == 1 {
                                let _ = ctx.http.delete_message(channel_id, chunk[0]).await;
                            } else {
                                let _ = ctx.http.delete_messages(channel_id, chunk).await;
                            }
                        }
                    }
                }
                if let Ok(resp) = ctx
                    .http
                    .create_message(channel_id)
                    .embeds(&[embed.clone()])
                    .await
                {
                    if let Ok(msg) = resp.model().await {
                        StatusMessageService::set(
                            ctx.clone(),
                            channel.guild_id,
                            channel.channel_id,
                            msg.id.get(),
                        )
                        .await;
                    }
                }
            }
        }
    }

    pub fn spawn(ctx: Arc<Context>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let token = shutdown::get_token();
            Self::update_all(ctx.clone()).await;
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = interval.tick() => {
                        Self::update_all(ctx.clone()).await;
                    }
                }
            }
        })
    }
}
