use std::time::Duration;

use deadpool_redis::Pool;
use reqwest::Client as ReqwestClient;
use twilight_cache_inmemory::{DefaultInMemoryCache, ResourceType};
use twilight_http::Client;

use crate::configs::discord::DISCORD_CONFIGS;
use crate::dbs::mongo::client::MongoDB;
use crate::dbs::redis::new_pool;

use super::Context;

pub struct ContextBuilder {
    http: Option<Client>,
    cache: Option<DefaultInMemoryCache>,
    redis: Option<Pool>,
    mongo: Option<MongoDB>,
    reqwest: Option<ReqwestClient>,
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextBuilder {
    pub fn new() -> Self {
        Self {
            http: None,
            cache: None,
            redis: None,
            mongo: None,
            reqwest: None,
        }
    }

    pub fn http(mut self, http: Client) -> Self {
        self.http = Some(http);
        self
    }

    pub fn cache(mut self, cache: DefaultInMemoryCache) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn redis(mut self, redis: Pool) -> Self {
        self.redis = Some(redis);
        self
    }

    pub fn mongo(mut self, mongo: MongoDB) -> Self {
        self.mongo = Some(mongo);
        self
    }

    pub fn reqwest(mut self, reqwest: ReqwestClient) -> Self {
        self.reqwest = Some(reqwest);
        self
    }

    pub async fn build(self) -> anyhow::Result<Context> {
        let http = self
            .http
            .unwrap_or_else(|| Client::new(DISCORD_CONFIGS.discord_token.clone()));

        let cache = self.cache.unwrap_or_else(|| {
            DefaultInMemoryCache::builder()
                .resource_types(
                    ResourceType::GUILD
                        | ResourceType::CHANNEL
                        | ResourceType::MESSAGE
                        | ResourceType::ROLE
                        | ResourceType::MEMBER
                        | ResourceType::USER_CURRENT,
                )
                .build()
        });

        let redis = self.redis.unwrap_or_else(new_pool);

        let mongo = match self.mongo {
            Some(mongo) => mongo,
            None => MongoDB::init(redis.clone()).await?,
        };

        let reqwest = match self.reqwest {
            Some(client) => client,
            None => ReqwestClient::builder()
                .pool_max_idle_per_host(10)
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .build()
                .expect("Failed to build Client"),
        };

        Ok(Context {
            http,
            cache,
            redis,
            mongo,
            reqwest,
        })
    }
}
