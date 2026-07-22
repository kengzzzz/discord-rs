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

pub async fn redis_exists(pool: &Pool, key: &str) -> bool {
    async {
        let mut conn = pool
            .get()
            .await
            .context("get redis connection")?;
        let exists = cmd("EXISTS")
            .arg(key)
            .query_async::<bool>(&mut conn)
            .await
            .context("execute EXISTS in redis")?;
        Ok::<bool, anyhow::Error>(exists)
    }
    .await
    .unwrap_or_else(|e| {
        tracing::error!(key, error = %e, "Redis EXISTS failed");
        // assume present so callers pruning by existence never drop a live key
        true
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

pub(crate) async fn redis_delete_prefixes_checked(
    pool: &Pool,
    prefixes: &[String],
) -> anyhow::Result<usize> {
    if prefixes.is_empty() {
        return Ok(0);
    }

    let mut conn = pool
        .get()
        .await
        .context("get redis connection")?;
    let mut cursor = 0_u64;
    let mut deleted = 0_usize;

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = cmd("SCAN")
            .arg(cursor)
            .arg("COUNT")
            .arg(256)
            .query_async(&mut conn)
            .await
            .context("scan Redis keys for prefix deletion")?;
        let matches: Vec<String> = keys
            .into_iter()
            .filter(|key| {
                prefixes
                    .iter()
                    .any(|prefix| key.starts_with(prefix))
            })
            .collect();

        if !matches.is_empty() {
            deleted += cmd("DEL")
                .arg(matches)
                .query_async::<usize>(&mut conn)
                .await
                .context("delete Redis keys matching prefixes")?;
        }

        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }

    Ok(deleted)
}

pub async fn redis_delete_prefixes(pool: &Pool, prefixes: &[String]) -> usize {
    redis_delete_prefixes_checked(pool, prefixes)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(?prefixes, error = %e, "Redis prefix deletion failed");
            0
        })
}
