use anyhow::Error as AnyError;
use chrono::Utc;
use deadpool_redis::Pool;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use twilight_http::{Error as HttpError, api_error::ApiError, error::ErrorType};
use twilight_model::{channel::Message, id::Id};

use crate::{
    context::Context,
    dbs::redis::{redis_delete, redis_get, redis_set_ex},
    services::{broadcast::BroadcastService, spam::quarantine},
};
use std::sync::Arc;

const SPAM_LIMIT: usize = 4;
const LOG_TTL: usize = 600;

#[derive(Serialize, Deserialize)]
struct SpamRecord {
    hash: String,
    histories: Vec<(u64, u64)>,
    timestamp: i64,
}

pub enum LogOutcome {
    None,
    NewlyQuarantined(String),
    AlreadyQuarantined,
}

pub async fn clear_log(pool: &Pool, guild_id: u64, user_id: u64) {
    let key = format!("spam:log:{guild_id}:{user_id}");
    redis_delete(pool, &key).await;
}

pub async fn log_message(ctx: &Arc<Context>, guild_id: u64, message: &Message) -> LogOutcome {
    let hash = hash_message(message).await;
    let key = format!("spam:log:{guild_id}:{}", message.author.id.get());
    let now = Utc::now().timestamp();
    let mut record = redis_get(&ctx.redis, &key)
        .await
        .unwrap_or(SpamRecord {
            hash: hash.clone(),
            histories: Vec::with_capacity(SPAM_LIMIT),
            timestamp: now,
        });

    if record.hash == hash
        && !record
            .histories
            .iter()
            .any(|h| h.0 == message.channel_id.get())
    {
        record
            .histories
            .push((message.channel_id.get(), message.id.get()));
    } else if record.hash != hash {
        record.hash = hash;
        record.histories.clear();
        record
            .histories
            .push((message.channel_id.get(), message.id.get()));
    }
    record.timestamp = now;

    if record.histories.len() >= SPAM_LIMIT {
        let to_delete = record.histories.clone();
        clear_log(&ctx.redis, guild_id, message.author.id.get()).await;
        BroadcastService::delete_replicas(ctx, &to_delete).await;
        let delete_ctx = ctx.clone();
        tokio::spawn(async move {
            for (c_id, m_id) in to_delete {
                if let Err(e) = delete_ctx
                    .http
                    .delete_message(Id::new(c_id), Id::new(m_id))
                    .await
                {
                    if is_unknown_message_error(&e) {
                        tracing::debug!(
                            channel_id = c_id,
                            message_id = m_id,
                            "spam message was already deleted"
                        );
                    } else {
                        tracing::warn!(channel_id = c_id, message_id = m_id, error = %e, "failed to delete spam message");
                    }
                }
            }
        });
        let token = format!("{:06}", fastrand::u32(0..1_000_000));
        return match quarantine::claim_token(ctx, guild_id, message.author.id.get(), &token).await {
            Ok(token) => LogOutcome::NewlyQuarantined(token),
            Err(Some(_)) => LogOutcome::AlreadyQuarantined,
            Err(None) => LogOutcome::AlreadyQuarantined,
        };
    }

    redis_set_ex(&ctx.redis, &key, &record, LOG_TTL).await;

    LogOutcome::None
}

async fn hash_message(message: &Message) -> String {
    let mut hasher = Sha256::new();
    hasher.update(message.content.as_bytes());
    for a in &message.attachments {
        hasher.update(&a.filename);
        hasher.update(a.size.to_be_bytes());
        if let Some(ct) = &a.content_type {
            hasher.update(ct);
        }
        if let Some(h) = &a.height {
            hasher.update(h.to_be_bytes());
        }
        if let Some(w) = a.width {
            hasher.update(w.to_be_bytes());
        }
    }
    hex::encode(hasher.finalize())
}

fn is_unknown_message_error(error: &AnyError) -> bool {
    matches!(
        error.downcast_ref::<HttpError>().map(HttpError::kind),
        Some(ErrorType::Response {
            error: ApiError::General(api_error),
            ..
        }) if api_error.code == 10008
    )
}

#[cfg(test)]
#[path = "tests/log.rs"]
mod tests;
