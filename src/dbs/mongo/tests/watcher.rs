use super::*;
use crate::dbs::redis::{new_pool, redis_exists};

fn resume_token(data: &str) -> ResumeToken {
    serde_json::from_value(serde_json::json!({ "_data": data })).expect("deserialize resume token")
}

fn command_error(code: i32) -> mongodb::error::Error {
    let command = mongodb::bson::from_document(mongodb::bson::doc! {
        "code": code,
        "codeName": "TestError",
        "errmsg": "test command error",
    })
    .expect("deserialize command error");
    ErrorKind::Command(command).into()
}

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

#[tokio::test]
async fn test_persist_resume_token_advances_without_an_event() {
    let pool = new_pool();
    let key = "changestream:resume:test_empty_batch";
    let old_token = resume_token("old");
    let new_token = resume_token("new");

    persist_resume_token(&pool, key, "test_empty_batch", Some(old_token)).await;
    persist_resume_token(
        &pool,
        key,
        "test_empty_batch",
        Some(new_token.clone()),
    )
    .await;

    assert_eq!(
        load_resume_token(&pool, key, "test_empty_batch").await,
        Some(new_token)
    );
    redis_delete(&pool, key).await;
}

#[test]
fn test_unusable_resume_token_errors_are_classified_by_code() {
    for code in [260, 280, 286] {
        assert!(is_unusable_resume_token(&command_error(code)));
    }
    assert!(!is_unusable_resume_token(&command_error(13)));
    assert!(!is_unusable_resume_token(
        &mongodb::error::Error::custom("message mentions resume token")
    ));
}
