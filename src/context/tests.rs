use super::*;
use once_cell::sync::OnceCell;

static REDIS_STARTED: OnceCell<()> = OnceCell::new();

async fn redis_start() {
    if REDIS_STARTED.get().is_some() {
        return;
    }
    unsafe {
        std::env::set_var("REDIS_URL", "redis://127.0.0.1:6379");
    }
    let _ = REDIS_STARTED.set(());
}

impl Context {
    pub async fn test() -> Self {
        redis_start().await;
        Self {
            http: Arc::new(Client::new(String::new())),
            cache: Arc::new(DefaultInMemoryCache::builder().build()),
            redis: new_pool(),
            mongo: MongoDB::empty().await,
            reqwest: ReqwestClient::new(),
        }
    }
}
