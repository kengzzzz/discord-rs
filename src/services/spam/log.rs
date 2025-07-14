use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use twilight_model::{channel::Message, id::Id};

use crate::{
    context::Context,
    dbs::redis::{redis_delete, redis_get, redis_set_ex},
    services::broadcast::BroadcastService,
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

pub async fn clear_log(guild_id: u64, user_id: u64) {
    let key = format!("spam:log:{guild_id}:{user_id}");
    redis_delete(&key).await;
}

pub async fn log_message(ctx: Arc<Context>, guild_id: u64, message: &Message) -> Option<String> {
    let hash = hash_message(message).await;
    let key = format!("spam:log:{guild_id}:{}", message.author.id.get());
    let now = Utc::now().timestamp();
    let mut record = redis_get::<SpamRecord>(&key).await.unwrap_or(SpamRecord {
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
        BroadcastService::delete_replicas(ctx.clone(), &to_delete).await;
        let http = ctx.http.clone();
        tokio::spawn(async move {
            for (c_id, m_id) in to_delete {
                if let Err(e) = http.delete_message(Id::new(c_id), Id::new(m_id)).await {
                    tracing::warn!(channel_id = c_id, message_id = m_id, error = %e, "failed to delete spam message");
                }
            }
        });
        let token = format!("{:06}", fastrand::u32(0..1_000_000));
        return Some(token);
    }

    redis_set_ex(&key, &record, LOG_TTL).await;

    None
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
