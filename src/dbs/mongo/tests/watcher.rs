use super::*;
use crate::dbs::redis::{new_pool, redis_exists};

#[tokio::test]
async fn test_load_resume_token_clears_invalid_token() {
    let pool = new_pool();
    let key = "changestream:resume:test_invalid";
    redis_set(&pool, key, &"not-a-resume-token".to_string()).await;

    let token = load_resume_token(&pool, key, "test_invalid").await;

    assert!(token.is_none());
    assert!(!redis_exists(&pool, key).await);
}

#[tokio::test]
async fn test_load_resume_token_keeps_valid_token() {
    let pool = new_pool();
    let key = "changestream:resume:test_valid";
    let valid = serde_json::to_string(&mongodb::bson::doc! { "_data": "8264A7C1" })
        .expect("serialize resume token");
    redis_set(&pool, key, &valid).await;

    let token = load_resume_token(&pool, key, "test_valid").await;

    assert!(token.is_some());
    assert!(redis_exists(&pool, key).await);
}

#[tokio::test]
async fn test_load_resume_token_missing_key() {
    let pool = new_pool();
    let key = "changestream:resume:test_missing";
    redis_delete(&pool, key).await;

    assert!(
        load_resume_token(&pool, key, "test_missing")
            .await
            .is_none()
    );
}
