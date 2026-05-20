use std::time::Duration;

use deadpool_redis::Pool;
use reqwest::Client as ReqwestClient;
use twilight_cache_inmemory::{DefaultInMemoryCache, ResourceType};
use twilight_http::Client as RawClient;

use crate::configs::discord::DISCORD_CONFIGS;
use crate::context::Context;
use crate::context::discord_http::Client;
use crate::dbs::mongo::MongoDB;
use crate::dbs::redis::new_pool;
use crate::services::{ai::AiService, scam_detect::ScamDetectQueue};

pub struct ContextBuilder {
    http: Option<RawClient>,
    cache: Option<DefaultInMemoryCache>,
    redis: Option<Pool>,
    mongo: Option<MongoDB>,
    reqwest: Option<ReqwestClient>,
    scam_detect: Option<ScamDetectQueue>,
    watchers: bool,
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
            scam_detect: None,
            watchers: true,
        }
    }

    pub fn http(mut self, http: RawClient) -> Self {
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

    pub fn scam_detect(mut self, scam_detect: ScamDetectQueue) -> Self {
        self.scam_detect = Some(scam_detect);
        self
    }

    pub fn watchers(mut self, watchers: bool) -> Self {
        self.watchers = watchers;
        self
    }

    pub async fn build(self) -> anyhow::Result<Context> {
        let http = self
            .http
            .unwrap_or_else(|| RawClient::new(DISCORD_CONFIGS.discord_token.clone()));
        let http = Client::new(http);

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
            None => MongoDB::init(redis.clone(), self.watchers).await?,
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

        let scam_detect = self
            .scam_detect
            .unwrap_or_else(ScamDetectQueue::from_env);

        Ok(Context {
            http,
            cache,
            redis,
            mongo,
            reqwest,
            ai_scheduler: AiService::scheduler(),
            scam_detect,
        })
    }
}
