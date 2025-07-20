use deadpool_redis::Pool;
use twilight_cache_inmemory::{DefaultInMemoryCache, ResourceType};

use crate::context::test_utils::mock_reqwest::MockReqwest;

use crate::context::test_utils::mock_context::Context;
use crate::context::test_utils::mock_http::MockClient as Client;
use crate::dbs::mongo::MongoDB;
use crate::dbs::redis::new_pool;

pub struct ContextBuilder {
    http: Option<Client>,
    cache: Option<DefaultInMemoryCache>,
    redis: Option<Pool>,
    mongo: Option<MongoDB>,
    reqwest: Option<MockReqwest>,
    watchers: bool,
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextBuilder {
    pub fn new() -> Self {
        Self { http: None, cache: None, redis: None, mongo: None, reqwest: None, watchers: true }
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

    pub fn reqwest(mut self, reqwest: MockReqwest) -> Self {
        self.reqwest = Some(reqwest);
        self
    }

    pub fn watchers(mut self, watchers: bool) -> Self {
        self.watchers = watchers;
        self
    }

    pub async fn build(self) -> anyhow::Result<Context> {
        let http = self.http.unwrap_or_default();

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

        let reqwest = self.reqwest.unwrap_or_default();

        Ok(Context { http, cache, redis, mongo, reqwest })
    }
}
