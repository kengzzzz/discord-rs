use super::*;
use crate::context::Context;
use crate::context::mock_http::MockClient as Client;
use crate::context::mock_reqwest::MockReqwest;
use crate::dbs::mongo::MongoDB;
use crate::dbs::redis::new_pool;
use twilight_cache_inmemory::DefaultInMemoryCache;

async fn test_context() -> Arc<Context> {
    let http = Client::new();
    let cache = DefaultInMemoryCache::new();
    let redis = new_pool();

    let mongo = MongoDB::init(redis.clone(), false).await.unwrap();

    let reqwest = MockReqwest::new();

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
