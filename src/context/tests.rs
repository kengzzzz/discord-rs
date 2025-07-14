use super::*;
use crate::tests::redis_setup;

impl Context {
    pub async fn test() -> Self {
        redis_setup::start().await;
        Self {
            http: Arc::new(Client::new(String::new())),
            cache: Arc::new(DefaultInMemoryCache::builder().build()),
            redis: new_pool(),
            mongo: MongoDB::empty().await,
            reqwest: Arc::new(ReqwestClient::new()),
        }
    }
}
