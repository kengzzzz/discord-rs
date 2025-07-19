mod builder;
pub use builder::ContextBuilder;

use deadpool_redis::Pool;
use reqwest::Client as ReqwestClient;
use twilight_cache_inmemory::DefaultInMemoryCache;
use twilight_http::Client;

use crate::dbs::mongo::MongoDB;
use crate::dbs::redis;

pub struct Context {
    pub http: Client,
    pub cache: DefaultInMemoryCache,
    pub redis: Pool,
    pub mongo: MongoDB,
    pub reqwest: ReqwestClient,
}

impl Context {
    pub async fn new() -> anyhow::Result<Self> {
        ContextBuilder::new().build().await
    }

    pub async fn redis_get<T>(&self, key: &str) -> Option<T>
    where
        T: serde::de::DeserializeOwned + Send + Sync,
    {
        redis::redis_get(&self.redis, key).await
    }

    pub async fn redis_set<T>(&self, key: &str, value: &T)
    where
        T: serde::Serialize + Sync,
    {
        redis::redis_set(&self.redis, key, value).await;
    }

    pub async fn redis_set_ex<T>(&self, key: &str, value: &T, ttl: usize)
    where
        T: serde::Serialize + Sync,
    {
        redis::redis_set_ex(&self.redis, key, value, ttl).await;
    }

    pub async fn redis_delete(&self, key: &str) {
        redis::redis_delete(&self.redis, key).await;
    }
}
