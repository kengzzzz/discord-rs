use deadpool_redis::{Config, Pool, Runtime};
use once_cell::sync::Lazy;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use tokio::sync::Mutex;

static REDIS_STORE: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn new_pool() -> Pool {
    let cfg = Config::default();
    cfg.create_pool(Some(Runtime::Tokio1)).unwrap()
}

pub async fn redis_get<T>(_pool: &Pool, key: &str) -> Option<T>
where
    T: DeserializeOwned + Send + Sync,
{
    let store = REDIS_STORE.lock().await;
    let json = store.get(key)?.clone();
    serde_json::from_str(&json).ok()
}

pub async fn redis_set<T>(_pool: &Pool, key: &str, value: &T)
where
    T: Serialize + Sync,
{
    let json = serde_json::to_string(value).expect("serialize value for redis_set_json");
    let mut store = REDIS_STORE.lock().await;
    store.insert(key.to_string(), json);
}

pub async fn redis_set_ex<T>(pool: &Pool, key: &str, value: &T, _ttl: usize)
where
    T: Serialize + Sync,
{
    redis_set(pool, key, value).await
}

pub async fn redis_delete(_pool: &Pool, key: &str) {
    let mut store = REDIS_STORE.lock().await;
    store.remove(key);
}
