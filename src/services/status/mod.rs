use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use once_cell::sync::Lazy;
use tokio::sync::watch;

use tokio::task::JoinHandle;
use twilight_model::id::Id;

use crate::services::shutdown;
use crate::{
    context::Context,
    dbs::mongo::models::channel::ChannelEnum,
    services::{channel::ChannelService, status_message::StatusMessageService},
};
use std::sync::Arc;

pub mod embed;

static UMBRA_FORMA: AtomicBool = AtomicBool::new(true);
static UMBRA_CHANNEL: Lazy<(watch::Sender<bool>, watch::Receiver<bool>)> =
    Lazy::new(|| watch::channel(true));

pub struct StatusService;

impl StatusService {
    pub fn subscribe_umbra_forma() -> watch::Receiver<bool> {
        UMBRA_CHANNEL.0.subscribe()
    }

    fn set_umbra_forma(val: bool) {
        let prev = UMBRA_FORMA.swap(val, Ordering::Relaxed);
        if prev != val {
            let _ = UMBRA_CHANNEL.0.send(val);
        }
    }

    pub fn is_umbra_forma() -> bool {
        UMBRA_FORMA.load(Ordering::Relaxed)
    }

    pub async fn update_all(ctx: &Arc<Context>) {
        let channels = ChannelService::list_by_type(ctx, &ChannelEnum::Status).await;
        for channel in channels {
            let Some(guild_ref) = ctx.cache.guild(Id::new(channel.guild_id)) else {
                continue;
            };
            let Some(embed) = embed::build_embed(ctx.clone(), &guild_ref).await else {
                continue;
            };
            let channel_id = Id::new(channel.channel_id);
            let mut existing = None;
            if let Some(record) = StatusMessageService::get(ctx, channel.guild_id).await {
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
                if let Err(e) = ctx
                    .http
                    .update_message(channel_id, Id::new(msg_id))
                    .embeds(Some(&[embed.clone()]))
                    .await
                {
                    tracing::warn!(channel_id = channel_id.get(), error = %e, "failed to update status message");
                }
                StatusMessageService::set(ctx, channel.guild_id, channel.channel_id, msg_id).await;
            } else {
                if let Ok(resp) = ctx.http.channel_messages(channel_id).await {
                    if let Ok(msgs) = resp.model().await {
                        let ids: Vec<_> = msgs.into_iter().map(|m| m.id).collect();

                        for chunk in ids.chunks(100) {
                            if chunk.len() == 1 {
                                if let Err(e) = ctx.http.delete_message(channel_id, chunk[0]).await
                                {
                                    tracing::warn!(channel_id = channel_id.get(), error = %e, "failed to delete old status message");
                                }
                            } else if let Err(e) = ctx.http.delete_messages(channel_id, chunk).await
                            {
                                tracing::warn!(channel_id = channel_id.get(), error = %e, "failed to bulk delete old status messages");
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
                            ctx,
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
            Self::update_all(&ctx).await;
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = interval.tick() => {
                        Self::update_all(&ctx).await;
                    }
                }
            }
        })
    }
}
