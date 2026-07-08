use anyhow::Context as _;
use deadpool_redis::{Config, Pool, Runtime, redis::cmd};
use serde::{Serialize, de::DeserializeOwned};

use crate::configs::redis::REDIS_CONFIGS;

pub fn new_pool() -> Pool {
    let cfg = Config::from_url(REDIS_CONFIGS.redis_url.clone());
    cfg.create_pool(Some(Runtime::Tokio1))
        .expect("create redis pool")
}

pub async fn redis_get<T>(pool: &Pool, key: &str) -> Option<T>
where
    T: DeserializeOwned + Send + Sync,
{
    let mut conn = pool.get().await.ok()?;

    let json: String = cmd("GET")
        .arg(key)
        .query_async(&mut conn)
        .await
        .ok()?;

    serde_json::from_str(&json)
        .context("redis_get_json: deserializing")
        .ok()
}

pub async fn redis_set<T>(pool: &Pool, key: &str, value: &T)
where
    T: Serialize + Sync,
{
    if let Err(e) = async {
        let json = serde_json::to_string(value).context("serialize value for redis_set_json")?;
        let mut conn = pool
            .get()
            .await
            .context("get redis connection")?;
        cmd("SET")
            .arg(key)
            .arg(json)
            .query_async::<()>(&mut conn)
            .await
            .context("execute SET in redis")?;
        Ok::<(), anyhow::Error>(())
    }
    .await
    {
        tracing::error!(key, error = %e, "Redis SET failed");
    }
}

pub async fn redis_set_ex<T>(pool: &Pool, key: &str, value: &T, ttl: usize)
where
    T: Serialize + Sync,
{
    if let Err(e) = async {
        let json = serde_json::to_string(value).context("serialize value for redis_set_ex")?;
        let mut conn = pool
            .get()
            .await
            .context("get redis connection")?;
        cmd("SET")
            .arg(key)
            .arg(json)
            .arg("EX")
            .arg(ttl)
            .query_async::<()>(&mut conn)
            .await
            .context("execute SET EX in redis")?;
        Ok::<(), anyhow::Error>(())
    }
    .await
    {
        tracing::error!(key, error = %e, "Redis SETEX failed");
    }
}

pub async fn redis_set_nx<T>(pool: &Pool, key: &str, value: &T) -> bool
where
    T: Serialize + Sync,
{
    async {
        let json = serde_json::to_string(value).context("serialize value for redis_set_nx")?;
        let mut conn = pool
            .get()
            .await
            .context("get redis connection")?;
        let was_set = cmd("SET")
            .arg(key)
            .arg(json)
            .arg("NX")
            .query_async::<bool>(&mut conn)
            .await
            .context("execute SET NX in redis")?;
        Ok::<bool, anyhow::Error>(was_set)
    }
    .await
    .unwrap_or_else(|e| {
        tracing::error!(key, error = %e, "Redis SET NX failed");
        false
    })
}

pub async fn redis_set_nx_ex<T>(pool: &Pool, key: &str, value: &T, ttl: usize) -> bool
where
    T: Serialize + Sync,
{
    async {
        let json = serde_json::to_string(value).context("serialize value for redis_set_nx_ex")?;
        let mut conn = pool
            .get()
            .await
            .context("get redis connection")?;
        let was_set = cmd("SET")
            .arg(key)
            .arg(json)
            .arg("NX")
            .arg("EX")
            .arg(ttl)
            .query_async::<bool>(&mut conn)
            .await
            .context("execute SET NX EX in redis")?;
        Ok::<bool, anyhow::Error>(was_set)
    }
    .await
    .unwrap_or_else(|e| {
        tracing::error!(key, error = %e, "Redis SET NX EX failed");
        false
    })
}

pub async fn redis_delete(pool: &Pool, key: &str) {
    if let Err(e) = async {
        let mut conn = pool.get().await?;
        cmd("DEL")
            .arg(key)
            .query_async::<()>(&mut conn)
            .await?;
        Ok::<_, anyhow::Error>(())
    }
    .await
    {
        tracing::error!(key, error = %e, "Redis DEL failed")
    }
}
