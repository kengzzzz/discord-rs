use deadpool_redis::{Config, Pool, Runtime};
use once_cell::sync::Lazy;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use tokio::sync::Mutex;

static REDIS_STORE: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static REDIS_TTLS: Lazy<Mutex<HashMap<String, usize>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn new_pool() -> Pool {
    let cfg = Config::default();
    cfg.create_pool(Some(Runtime::Tokio1))
        .unwrap()
}

pub async fn redis_get<T>(_pool: &Pool, key: &str) -> Option<T>
where
    T: DeserializeOwned + Send + Sync,
{
    tokio::task::yield_now().await;
    let store = REDIS_STORE.lock().await;
    let json = store.get(key)?.clone();
    serde_json::from_str(&json).ok()
}

pub async fn redis_set<T>(_pool: &Pool, key: &str, value: &T)
where
    T: Serialize + Sync,
{
    tokio::task::yield_now().await;
    let json = serde_json::to_string(value).expect("serialize value for redis_set_json");
    let mut store = REDIS_STORE.lock().await;
    store.insert(key.to_string(), json);
    let mut ttls = REDIS_TTLS.lock().await;
    ttls.remove(key);
}

pub async fn redis_set_ex<T>(pool: &Pool, key: &str, value: &T, ttl: usize)
where
    T: Serialize + Sync,
{
    redis_set(pool, key, value).await;
    let mut ttls = REDIS_TTLS.lock().await;
    ttls.insert(key.to_string(), ttl);
}

pub async fn redis_set_nx<T>(_pool: &Pool, key: &str, value: &T) -> bool
where
    T: Serialize + Sync,
{
    let json = serde_json::to_string(value).expect("serialize value for redis_set_nx");
    let mut store = REDIS_STORE.lock().await;
    if store.contains_key(key) {
        return false;
    }
    store.insert(key.to_string(), json);
    let mut ttls = REDIS_TTLS.lock().await;
    ttls.remove(key);
    true
}

pub async fn redis_set_nx_ex<T>(pool: &Pool, key: &str, value: &T, ttl: usize) -> bool
where
    T: Serialize + Sync,
{
    let was_set = redis_set_nx(pool, key, value).await;
    if was_set {
        let mut ttls = REDIS_TTLS.lock().await;
        ttls.insert(key.to_string(), ttl);
    }
    was_set
}

pub async fn redis_exists(_pool: &Pool, key: &str) -> bool {
    tokio::task::yield_now().await;
    let store = REDIS_STORE.lock().await;
    store.contains_key(key)
}

pub async fn redis_delete(_pool: &Pool, key: &str) {
    let mut store = REDIS_STORE.lock().await;
    store.remove(key);
    let mut ttls = REDIS_TTLS.lock().await;
    ttls.remove(key);
}

pub async fn redis_delete_prefixes(_pool: &Pool, prefixes: &[String]) -> usize {
    let mut store = REDIS_STORE.lock().await;
    let before = store.len();
    store.retain(|key, _| {
        !prefixes
            .iter()
            .any(|prefix| key.starts_with(prefix))
    });
    let deleted = before - store.len();

    let mut ttls = REDIS_TTLS.lock().await;
    ttls.retain(|key, _| {
        !prefixes
            .iter()
            .any(|prefix| key.starts_with(prefix))
    });
    deleted
}

pub async fn redis_ttl(key: &str) -> Option<usize> {
    let ttls = REDIS_TTLS.lock().await;
    ttls.get(key).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn delete_prefixes_removes_only_matching_keys_and_ttls() {
        let pool = new_pool();
        let prefix = "test:prefix-delete:matching:";
        let other_key = "test:prefix-delete:other";
        let resume_key = "changestream:resume:test-prefix-delete";
        let matching_keys = [format!("{prefix}one"), format!("{prefix}two")];

        for key in &matching_keys {
            redis_set_ex(&pool, key, &"value", 60).await;
        }
        redis_set_ex(&pool, other_key, &"other", 60).await;
        redis_set(&pool, resume_key, &"token").await;

        let deleted = redis_delete_prefixes(&pool, &[prefix.to_owned()]).await;

        assert_eq!(deleted, matching_keys.len());
        for key in &matching_keys {
            assert!(!redis_exists(&pool, key).await);
            assert_eq!(redis_ttl(key).await, None);
        }
        assert!(redis_exists(&pool, other_key).await);
        assert!(redis_exists(&pool, resume_key).await);

        redis_delete(&pool, other_key).await;
        redis_delete(&pool, resume_key).await;
    }
}
