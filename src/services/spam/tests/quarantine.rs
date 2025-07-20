use super::*;
use crate::context::{ContextBuilder, mock_http::MockClient as Client};

async fn build_context() -> Arc<Context> {
    let ctx = ContextBuilder::new()
        .http(Client::new())
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
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
