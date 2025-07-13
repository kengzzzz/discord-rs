use once_cell::sync::Lazy;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use tokio::sync::RwLock;

pub(super) static REDIS_STORE: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub async fn redis_get<T>(key: &str) -> Option<T>
where
    T: DeserializeOwned + Send + Sync,
{
    let map = REDIS_STORE.read().await;
    let json = map.get(key)?.clone();
    serde_json::from_str(&json).ok()
}

pub async fn redis_set<T>(key: &str, value: &T)
where
    T: Serialize + Sync,
{
    if let Ok(json) = serde_json::to_string(value) {
        REDIS_STORE.write().await.insert(key.to_string(), json);
    }
}

pub async fn redis_set_ex<T>(key: &str, value: &T, _ttl: usize)
where
    T: Serialize + Sync,
{
    redis_set(key, value).await;
}

pub async fn redis_delete(key: &str) {
    REDIS_STORE.write().await.remove(key);
}
