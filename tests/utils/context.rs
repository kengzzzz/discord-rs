use discord_bot::context::Context;
use discord_bot::services::shutdown;

use super::redis_setup;

#[allow(dead_code)]
pub async fn test_context() -> Context {
    redis_setup::start().await;
    shutdown::set_token(tokio_util::sync::CancellationToken::new());
    #[cfg(feature = "test-utils")]
    {
        Context::test().await
    }
    #[cfg(not(feature = "test-utils"))]
    {
        use discord_bot::dbs::{mongo::client::MongoDB, redis::new_pool};
        use reqwest::Client as ReqwestClient;
        use std::sync::Arc;
        use twilight_cache_inmemory::DefaultInMemoryCache;
        use twilight_http::Client;

        Context {
            http: Arc::new(Client::new(String::new())),
            cache: Arc::new(DefaultInMemoryCache::builder().build()),
            redis: new_pool(),
            mongo: MongoDB::init(new_pool()).await.unwrap(),
            reqwest: Arc::new(ReqwestClient::new()),
        }
    }
}
