use deadpool_redis::Pool;
use mongodb::bson::{doc, to_bson};
use twilight_model::id::{
    Id,
    marker::{GuildMarker, UserMarker},
};

use crate::{
    context::Context,
    dbs::{
        mongo::models::{quarantine::Quarantine, role::RoleEnum},
        redis::{redis_delete, redis_get, redis_set},
    },
    services::role::RoleService,
};
use std::sync::Arc;

pub async fn verify(
    ctx: &Arc<Context>,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    token: &str,
) -> bool {
    let key = format!("spam:quarantine:{}:{}", guild_id.get(), user_id.get());

    if let Some(stored) = redis_get::<String>(&ctx.redis, &key).await {
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
            RoleService::get_by_type(ctx, guild_id.get(), &RoleEnum::Quarantine).await
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

pub async fn get_token(ctx: &Arc<Context>, guild_id: u64, user_id: u64) -> Option<String> {
    let key = format!("spam:quarantine:{guild_id}:{user_id}");
    if let Some(token) = redis_get::<String>(&ctx.redis, &key).await {
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

    redis_set(&ctx.redis, &key, &token).await;

    token
}

pub async fn quarantine_member(
    ctx: &Arc<Context>,
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
            RoleService::get_by_type(ctx, guild_id.get(), &RoleEnum::Quarantine).await
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

pub async fn purge_cache(pool: &Pool, guild_id: u64, user_id: u64) {
    let log_key = format!("spam:log:{guild_id}:{user_id}");
    let quarantine_key = format!("spam:quarantine:{guild_id}:{user_id}");
    redis_delete(pool, &log_key).await;
    redis_delete(pool, &quarantine_key).await;
}

#[cfg(all(test, not(feature = "test-utils")))]
mod tests {
    use super::*;
    use crate::{
        context::mock_http::MockClient as Client,
        dbs::{mongo::MongoDB, redis::new_pool},
    };
    use tokio::sync::OnceCell;
    use twilight_cache_inmemory::InMemoryCache;

    async fn build_context() -> Arc<Context> {
        static CTX: OnceCell<Arc<Context>> = OnceCell::const_new();
        CTX.get_or_init(|| async {
            unsafe {
                std::env::set_var("REDIS_URL", "redis://127.0.0.1:6379");
            }
            let http = Client::new();
            let cache = InMemoryCache::builder().build();
            let redis = new_pool();
            let mongo = MongoDB::init(redis.clone(), false).await.unwrap();
            let reqwest = reqwest::Client::new();
            Arc::new(Context {
                http,
                cache,
                redis,
                mongo,
                reqwest,
            })
        })
        .await
        .clone()
    }

    #[tokio::test]
    async fn test_get_token_from_redis() {
        let ctx = build_context().await;
        let key = "spam:quarantine:1:1";
        redis_set(&ctx.redis, key, &"redis_token").await;
        let record = Quarantine {
            id: None,
            guild_id: 1,
            user_id: 1,
            token: "mongo_token".into(),
            roles: Vec::new(),
        };
        ctx.mongo.quarantines.insert_one(record).await.unwrap();
        let token = get_token(&ctx, 1, 1).await;
        assert_eq!(token, Some("redis_token".into()));
    }

    #[tokio::test]
    async fn test_get_token_fallback_to_mongo() {
        let ctx = build_context().await;
        let record = Quarantine {
            id: None,
            guild_id: 1,
            user_id: 2,
            token: "mongo_token".into(),
            roles: Vec::new(),
        };
        ctx.mongo.quarantines.insert_one(record).await.unwrap();
        let token = get_token(&ctx, 1, 2).await;
        assert_eq!(token, Some("mongo_token".into()));
        let key = "spam:quarantine:1:2";
        let cached: String = redis_get(&ctx.redis, key).await.unwrap();
        assert_eq!(cached, "mongo_token");
    }

    #[tokio::test]
    async fn test_purge_cache() {
        let ctx = build_context().await;
        let log_key = "spam:log:1:3";
        let quarantine_key = "spam:quarantine:1:3";
        redis_set(&ctx.redis, log_key, &1).await;
        redis_set(&ctx.redis, quarantine_key, &"tok").await;
        purge_cache(&ctx.redis, 1, 3).await;
        let log: Option<i32> = redis_get(&ctx.redis, log_key).await;
        let quarantine: Option<String> = redis_get(&ctx.redis, quarantine_key).await;
        assert!(log.is_none());
        assert!(quarantine.is_none());
    }

    #[tokio::test]
    async fn test_verify_success_and_delete_record() {
        let ctx = build_context().await;
        let record = Quarantine {
            id: None,
            guild_id: 1,
            user_id: 4,
            token: "token".into(),
            roles: Vec::new(),
        };
        ctx.mongo.quarantines.insert_one(record).await.unwrap();
        redis_set(&ctx.redis, "spam:quarantine:1:4", &"token").await;
        let ok = verify(&ctx, Id::new(1), Id::new(4), "token").await;
        assert!(ok);
        let remaining = ctx
            .mongo
            .quarantines
            .find_one(doc! {"guild_id": 1i64, "user_id": 4i64})
            .await
            .unwrap();
        assert!(remaining.is_none());
    }

    #[tokio::test]
    async fn test_verify_fails_on_mismatched_token() {
        let ctx = build_context().await;
        let record = Quarantine {
            id: None,
            guild_id: 1,
            user_id: 5,
            token: "token".into(),
            roles: Vec::new(),
        };
        ctx.mongo.quarantines.insert_one(record).await.unwrap();
        redis_set(&ctx.redis, "spam:quarantine:1:5", &"other").await;
        let ok = verify(&ctx, Id::new(1), Id::new(5), "token").await;
        assert!(!ok);
        let remaining = ctx
            .mongo
            .quarantines
            .find_one(doc! {"guild_id": 1i64, "user_id": 5i64})
            .await
            .unwrap();
        assert!(remaining.is_some());
    }
}
