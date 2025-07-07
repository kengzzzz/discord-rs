use chrono::Utc;
use futures::StreamExt;
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
    configs::discord::{CACHE, HTTP},
    dbs::{
        mongo::{mongodb::MongoDB, quarantine::Quarantine, role::RoleEnum},
        redis::{redis_delete, redis_get, redis_set, redis_set_ex},
    },
    services::{broadcast::BroadcastService, role::RoleService},
};

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
    pub async fn is_quarantined(guild_id: u64, user_id: u64) -> bool {
        let key = format!("spam:quarantine:{guild_id}:{user_id}");
        if redis_get::<String>(&key).await.is_some() {
            return true;
        }

        let res = MongoDB::get()
            .quarantines
            .find_one(doc! {"guild_id": guild_id as i64, "user_id": user_id as i64})
            .await
            .ok()
            .flatten();

        redis_set(&key, &res).await;

        res.is_some()
    }

    pub async fn verify(guild_id: Id<GuildMarker>, user_id: Id<UserMarker>, token: &str) -> bool {
        let key = format!("spam:quarantine:{}:{}", guild_id.get(), user_id.get());

        if let Some(stored) = redis_get::<String>(&key).await {
            if stored != token {
                return false;
            }
        }

        let db = MongoDB::get();
        if let Ok(Some(record)) = db
            .quarantines
            .find_one(doc! {
                "guild_id": guild_id.get() as i64,
                "user_id": user_id.get() as i64,
                "token": token,
            })
            .await
        {
            if let Some(role) =
                RoleService::get_by_type(guild_id.get(), &RoleEnum::Quarantine).await
            {
                let _ = HTTP
                    .remove_guild_member_role(guild_id, user_id, Id::new(role.role_id))
                    .await;
            }
            for id in record.roles.iter() {
                let _ = HTTP
                    .add_guild_member_role(guild_id, user_id, Id::new(*id))
                    .await;
            }

            let _ = db
                .quarantines
                .delete_one(doc! {
                    "guild_id": guild_id.get() as i64,
                    "user_id": user_id.get() as i64,
                })
                .await;

            return true;
        }

        false
    }

    pub async fn get_token(guild_id: u64, user_id: u64) -> Option<String> {
        let key = format!("spam:quarantine:{guild_id}:{user_id}");
        if let Some(token) = redis_get::<String>(&key).await {
            return Some(token);
        }

        let db = MongoDB::get();
        let token = db
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
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        token: &str,
    ) {
        if let Some(member_ref) = CACHE.member(guild_id, user_id) {
            let roles = member_ref.roles();
            for r in roles {
                let _ = HTTP.remove_guild_member_role(guild_id, user_id, *r).await;
            }
            if let Some(role) =
                RoleService::get_by_type(guild_id.get(), &RoleEnum::Quarantine).await
            {
                let _ = HTTP
                    .add_guild_member_role(guild_id, user_id, Id::new(role.role_id))
                    .await;
            }
            let record = Quarantine {
                id: None,
                guild_id: guild_id.get(),
                user_id: user_id.get(),
                token: token.to_string(),
                roles: roles.iter().map(|r| r.get()).collect(),
            };
            if let Ok(bson) = to_bson(&record) {
                let _ = MongoDB::get()
                    .quarantines
                    .update_one(
                        doc! {"guild_id": record.guild_id as i64, "user_id": record.user_id as i64},
                        doc! {"$set": bson},
                    )
                    .upsert(true)
                    .await;
            }
        }
    }

    pub async fn log_message(guild_id: u64, message: &Message) -> Option<String> {
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
            BroadcastService::delete_replicas(&to_delete).await;
            tokio::spawn(async move {
                for (c_id, m_id) in to_delete {
                    let _ = HTTP.delete_message(Id::new(c_id), Id::new(m_id)).await;
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
            match reqwest::get(&a.url).await {
                Ok(resp) => {
                    let mut stream = resp.bytes_stream();
                    let mut errored = false;
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => hasher.update(&bytes),
                            Err(_) => {
                                errored = true;
                                break;
                            }
                        }
                    }
                    if errored {
                        hasher.update(a.url.as_bytes());
                    }
                }
                Err(_) => hasher.update(a.url.as_bytes()),
            }
        }
        hex::encode(hasher.finalize())
    }
}
