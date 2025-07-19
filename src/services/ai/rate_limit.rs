use std::sync::Arc;

use chrono::Utc;
use twilight_model::id::{Id, marker::UserMarker};

use crate::{configs::CACHE_PREFIX, context::Context};

const RATE_LIMIT_SECS: i64 = 3;

pub(crate) async fn check_rate_limit(ctx: &Arc<Context>, user: Id<UserMarker>) -> Option<u64> {
    let key = format!("{CACHE_PREFIX}:ai:rate:{}", user.get());
    let now = Utc::now().timestamp();
    if let Some(last) = ctx.redis_get::<i64>(&key).await {
        let diff = now - last;
        if diff < RATE_LIMIT_SECS {
            return Some((RATE_LIMIT_SECS - diff) as u64);
        }
    }
    ctx.redis_set_ex(&key, &now, RATE_LIMIT_SECS as usize).await;
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;
    use crate::dbs::mongo::models::{
        ai_prompt::AiPrompt, channel::Channel, message::Message, quarantine::Quarantine, role::Role,
    };
    use crate::dbs::redis::new_pool;
    use mongodb::Client;
    use reqwest::Client as ReqwestClient;
    use twilight_cache_inmemory::DefaultInMemoryCache;
    use twilight_http::Client as HttpClient;

    async fn test_context() -> Arc<Context> {
        let http = HttpClient::new(String::new());
        let cache = DefaultInMemoryCache::new();
        let redis = new_pool();

        let mongo_client = Client::with_uri_str("mongodb://localhost:27017")
            .await
            .unwrap();
        let database = mongo_client.database("test_db");
        let mongo = crate::dbs::mongo::client::MongoDB {
            client: mongo_client,
            channels: database.collection::<Channel>("channels"),
            roles: database.collection::<Role>("roles"),
            quarantines: database.collection::<Quarantine>("quarantines"),
            messages: database.collection::<Message>("messages"),
            ai_prompts: database.collection::<AiPrompt>("ai_prompts"),
        };

        let reqwest = ReqwestClient::new();

        Arc::new(Context {
            http,
            cache,
            redis,
            mongo,
            reqwest,
        })
    }

    #[tokio::test]
    async fn test_returns_wait_time_when_called_too_fast() {
        let ctx = test_context().await;
        let user = Id::<UserMarker>::new(1);

        let key = format!("{CACHE_PREFIX}:ai:rate:{}", user.get());
        let now = Utc::now().timestamp();
        ctx.redis_set_ex(&key, &(now - 1), RATE_LIMIT_SECS as usize)
            .await;

        let wait = check_rate_limit(&ctx, user).await;
        assert_eq!(wait, Some((RATE_LIMIT_SECS - 1) as u64));

        let stored = ctx.redis_get::<i64>(&key).await.unwrap();
        assert_eq!(stored, now - 1);
    }

    #[tokio::test]
    async fn test_stores_timestamp_on_success() {
        let ctx = test_context().await;
        let user = Id::<UserMarker>::new(2);

        let wait = check_rate_limit(&ctx, user).await;
        assert_eq!(wait, None);

        let key = format!("{CACHE_PREFIX}:ai:rate:{}", user.get());
        let stored = ctx.redis_get::<i64>(&key).await;
        assert!(stored.is_some());
    }
}
