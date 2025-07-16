use std::sync::Arc;
use std::time::Duration;

use deadpool_redis::Pool;
use reqwest::Client as ReqwestClient;
use twilight_cache_inmemory::{DefaultInMemoryCache, ResourceType};
use twilight_http::Client;

use crate::configs::discord::DISCORD_CONFIGS;
use crate::dbs::mongo::client::MongoDB;
use crate::dbs::redis::{self, new_pool};

#[derive(Clone)]
pub struct Context {
    pub http: Arc<Client>,
    pub cache: Arc<DefaultInMemoryCache>,
    pub redis: Pool,
    pub mongo: Arc<MongoDB>,
    pub reqwest: Arc<ReqwestClient>,
}

impl Context {
    pub async fn new() -> anyhow::Result<Self> {
        let http = Arc::new(Client::new(DISCORD_CONFIGS.discord_token.clone()));
        let cache = Arc::new(
            DefaultInMemoryCache::builder()
                .resource_types(
                    ResourceType::GUILD
                        | ResourceType::CHANNEL
                        | ResourceType::MESSAGE
                        | ResourceType::ROLE
                        | ResourceType::MEMBER
                        | ResourceType::USER_CURRENT,
                )
                .build(),
        );

        let redis = new_pool();

        let mongo = MongoDB::init(redis.clone()).await?;

        let reqwest = Arc::new(
            ReqwestClient::builder()
                .pool_max_idle_per_host(10)
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .build()
                .expect("Failed to build Client"),
        );

        Ok(Self {
            http,
            cache,
            redis,
            mongo,
            reqwest,
        })
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

#[cfg(any(test, feature = "test-utils"))]
pub mod tests;
