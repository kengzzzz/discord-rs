use chrono::Utc;
use mongodb::bson::{doc, to_bson};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use twilight_model::{
    channel::Message,
    id::{
        Id,
        marker::{GuildMarker, UserMarker},
    },
};

use crate::{
    context::Context,
    dbs::{
        mongo::{quarantine::Quarantine, role::RoleEnum},
        redis::{redis_delete, redis_get, redis_set, redis_set_ex},
    },
    services::{broadcast::BroadcastService, role::RoleService},
};
use std::sync::Arc;

pub struct SpamService;

const SPAM_LIMIT: usize = 4;
const LOG_TTL: usize = 600;

#[derive(Serialize, Deserialize)]
struct SpamRecord {
    hash: String,
    histories: Vec<(u64, u64)>,
    timestamp: i64,
}

impl SpamService {
    pub async fn is_quarantined(ctx: Arc<Context>, guild_id: u64, user_id: u64) -> bool {
        let key = format!("spam:quarantine:{guild_id}:{user_id}");
        if redis_get::<String>(&key).await.is_some() {
            return true;
        }

        let res = ctx
            .mongo
            .quarantines
            .find_one(doc! {"guild_id": guild_id as i64, "user_id": user_id as i64})
            .await
            .ok()
            .flatten();

        redis_set(&key, &res).await;

        res.is_some()
    }

    pub async fn verify(
        ctx: Arc<Context>,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        token: &str,
    ) -> bool {
        let key = format!("spam:quarantine:{}:{}", guild_id.get(), user_id.get());

        if let Some(stored) = redis_get::<String>(&key).await {
            if stored != token {
                return false;
            }
        }

        if let Ok(Some(record)) = ctx
            .mongo
            .quarantines
            .find_one(doc! {
                "guild_id": guild_id.get() as i64,
                "user_id": user_id.get() as i64,
                "token": token,
            })
            .await
        {
            if let Some(role) =
                RoleService::get_by_type(ctx.clone(), guild_id.get(), &RoleEnum::Quarantine).await
            {
                if let Err(e) = ctx
                    .http
                    .remove_guild_member_role(guild_id, user_id, Id::new(role.role_id))
                    .await
                {
                    tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), error = %e, "failed to remove quarantine role");
                }
            }
            for id in record.roles.iter() {
                if let Err(e) = ctx
                    .http
                    .add_guild_member_role(guild_id, user_id, Id::new(*id))
                    .await
                {
                    tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = *id, error = %e, "failed to restore member role");
                }
            }

            if let Err(e) = ctx
                .mongo
                .quarantines
                .delete_one(doc! {
                    "guild_id": guild_id.get() as i64,
                    "user_id": user_id.get() as i64,
                })
                .await
            {
                tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), error = %e, "failed to delete quarantine record");
            }

            return true;
        }

        false
    }

    pub async fn get_token(ctx: Arc<Context>, guild_id: u64, user_id: u64) -> Option<String> {
        let key = format!("spam:quarantine:{guild_id}:{user_id}");
        if let Some(token) = redis_get::<String>(&key).await {
            return Some(token);
        }

        let token = ctx
            .mongo
            .quarantines
            .find_one(doc! {"guild_id": guild_id as i64, "user_id": user_id as i64})
            .await
            .ok()
            .flatten()
            .map(|r| r.token);

        redis_set(&key, &token).await;

        token
    }

    pub async fn clear_log(guild_id: u64, user_id: u64) {
        let key = format!("spam:log:{guild_id}:{user_id}");
        redis_delete(&key).await;
    }

    pub async fn quarantine_member(
        ctx: Arc<Context>,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        token: &str,
    ) {
        if let Some(member_ref) = ctx.cache.member(guild_id, user_id) {
            let roles = member_ref.roles();
            for r in roles {
                if let Err(e) = ctx
                    .http
                    .remove_guild_member_role(guild_id, user_id, *r)
                    .await
                {
                    tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = r.get(), error = %e, "failed to remove member role for quarantine");
                }
            }
            if let Some(role) =
                RoleService::get_by_type(ctx.clone(), guild_id.get(), &RoleEnum::Quarantine).await
            {
                if let Err(e) = ctx
                    .http
                    .add_guild_member_role(guild_id, user_id, Id::new(role.role_id))
                    .await
                {
                    tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = role.role_id, error = %e, "failed to assign quarantine role");
                }
            }
            let record = Quarantine {
                id: None,
                guild_id: guild_id.get(),
                user_id: user_id.get(),
                token: token.to_string(),
                roles: roles.iter().map(|r| r.get()).collect(),
            };
            if let Ok(bson) = to_bson(&record) {
                if let Err(e) = ctx
                    .mongo
                    .quarantines
                    .update_one(
                        doc! {"guild_id": record.guild_id as i64, "user_id": record.user_id as i64},
                        doc! {"$set": bson},
                    )
                    .upsert(true)
                    .await
                {
                    tracing::warn!(guild_id = record.guild_id, user_id = record.user_id, error = %e, "failed to upsert quarantine record");
                }
            }
        }
    }

    pub async fn log_message(
        ctx: Arc<Context>,
        guild_id: u64,
        message: &Message,
    ) -> Option<String> {
        let hash = Self::hash_message(message).await;
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

    pub async fn purge_cache(guild_id: u64, user_id: u64) {
        let log_key = format!("spam:log:{guild_id}:{user_id}");
        let quarantine_key = format!("spam:quarantine:{guild_id}:{user_id}");
        redis_delete(&log_key).await;
        redis_delete(&quarantine_key).await;
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
}
