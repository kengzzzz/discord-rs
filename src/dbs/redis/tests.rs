#![cfg(feature = "mock-redis")]

use deadpool_redis::Pool;
use once_cell::sync::Lazy;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use tokio::sync::RwLock;

pub(super) static REDIS_STORE: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub async fn redis_get<T>(_pool: &Pool, key: &str) -> Option<T>
where
    T: DeserializeOwned + Send + Sync,
{
    let map = REDIS_STORE.read().await;
    let json = map.get(key)?.clone();
    serde_json::from_str(&json).ok()
}

pub async fn redis_set<T>(_pool: &Pool, key: &str, value: &T)
where
    T: Serialize + Sync,
{
    if let Ok(json) = serde_json::to_string(value) {
        REDIS_STORE.write().await.insert(key.to_string(), json);
    }
}

pub async fn redis_set_ex<T>(pool: &Pool, key: &str, value: &T, _ttl: usize)
where
    T: Serialize + Sync,
{
    redis_set(pool, key, value).await;
}

pub async fn redis_delete(_pool: &Pool, key: &str) {
    REDIS_STORE.write().await.remove(key);
}
