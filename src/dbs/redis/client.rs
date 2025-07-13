#[allow(unused_imports)]
use anyhow::Context as _;
#[allow(unused_imports)]
use deadpool_redis::{Config, Pool, Runtime, redis::cmd};
use once_cell::sync::Lazy;
#[allow(unused_imports)]
use serde::{Serialize, de::DeserializeOwned};

use crate::configs::redis::REDIS_CONFIGS;

pub static REDIS_POOL: Lazy<Pool> = Lazy::new(new_pool);

pub fn new_pool() -> Pool {
    let cfg = Config::from_url(REDIS_CONFIGS.redis_url.clone());
    cfg.create_pool(Some(Runtime::Tokio1))
        .expect("create redis pool")
}

#[cfg(not(test))]
pub async fn redis_get<T>(key: &str) -> Option<T>
where
    T: DeserializeOwned + Send + Sync,
{
    let pool: &Pool = &REDIS_POOL;
    let mut conn = pool.get().await.ok()?;

    let json: String = cmd("GET").arg(key).query_async(&mut conn).await.ok()?;

    serde_json::from_str(&json)
        .context("redis_get_json: deserializing")
        .ok()
}

#[cfg(not(test))]
pub async fn redis_set<T>(key: &str, value: &T)
where
    T: Serialize + Sync,
{
    if let Err(e) = async {
        let json = serde_json::to_string(value).context("serialize value for redis_set_json")?;
        let mut conn = REDIS_POOL.get().await.context("get redis connection")?;
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

#[cfg(not(test))]
pub async fn redis_set_ex<T>(key: &str, value: &T, ttl: usize)
where
    T: Serialize + Sync,
{
    if let Err(e) = async {
        let json = serde_json::to_string(value).context("serialize value for redis_set_ex")?;
        let mut conn = REDIS_POOL.get().await.context("get redis connection")?;
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

#[cfg(not(test))]
pub async fn redis_delete(key: &str) {
    if let Err(e) = async {
        let pool: &Pool = &REDIS_POOL;
        let mut conn = pool.get().await?;
        cmd("DEL").arg(key).query_async::<()>(&mut conn).await?;
        Ok::<_, anyhow::Error>(())
    }
    .await
    {
        tracing::error!(key, error = %e, "Redis DEL failed")
    }
}
