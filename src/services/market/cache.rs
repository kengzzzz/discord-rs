use std::{
    cmp,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{
    context::Context,
    utils::ascii::{cmp_ignore_ascii_case, collect_prefix_icase},
};

use super::{MarketService, client};
use std::sync::Arc;

const REDIS_KEY: &str = "discord-bot:market-items";
const UPDATE_SECS: u16 = 60 * 60;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct MarketEntry {
    pub name: String,
    pub url: String,
}

static ITEMS: Lazy<RwLock<Vec<MarketEntry>>> = Lazy::new(|| RwLock::new(Vec::new()));
static LAST_UPDATE: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

impl MarketService {
    async fn set_items(data: Vec<MarketEntry>) {
        *ITEMS.write().await = data;
    }
    pub async fn init(ctx: Arc<Context>) {
        if let Some(data) = client::load_from_redis(&ctx.redis, REDIS_KEY).await {
            Self::set_items(data).await;
            LAST_UPDATE.store(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                Ordering::Relaxed,
            );
        } else if let Err(e) =
            client::update_items(&ctx.reqwest, REDIS_KEY, &ITEMS, &LAST_UPDATE, &ctx.redis).await
        {
            tracing::warn!(error = %e, "failed to update market items");
        }
    }

    pub async fn search(prefix: &str) -> Vec<String> {
        let items = ITEMS.read().await;
        if items.is_empty() {
            return Vec::new();
        }
        collect_prefix_icase(&items, prefix, |e| &e.name)
    }

    async fn maybe_refresh(ctx: &Arc<Context>) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_UPDATE.load(Ordering::Relaxed);
        if now.saturating_sub(last) > UPDATE_SECS as u64 {
            if let Err(e) =
                client::update_items(&ctx.reqwest, REDIS_KEY, &ITEMS, &LAST_UPDATE, &ctx.redis)
                    .await
            {
                tracing::warn!(error = %e, "failed to update market items");
            }
        }
    }

    pub async fn search_with_update(ctx: &Arc<Context>, prefix: &str) -> Vec<String> {
        let mut results = Self::search(prefix).await;
        if results.is_empty() {
            Self::maybe_refresh(ctx).await;
            results = Self::search(prefix).await;
        }
        results
    }

    pub(super) async fn find_url(name: &str) -> Option<String> {
        let items = ITEMS.read().await;
        let idx =
            items.partition_point(|e| cmp_ignore_ascii_case(&e.name, name) == cmp::Ordering::Less);
        if idx < items.len()
            && cmp_ignore_ascii_case(&items[idx].name, name) == cmp::Ordering::Equal
        {
            Some(items[idx].url.clone())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dbs::mongo::client::MongoDB,
        dbs::mongo::models::{
            ai_prompt::AiPrompt, channel::Channel, message::Message, quarantine::Quarantine,
            role::Role,
        },
        dbs::redis::new_pool,
    };
    use tokio::sync::OnceCell;

    async fn build_context() -> Arc<Context> {
        static CTX: OnceCell<Arc<Context>> = OnceCell::const_new();
        CTX.get_or_init(|| async {
            unsafe {
                std::env::set_var("REDIS_URL", "redis://127.0.0.1:6379");
            }
            let http = twilight_http::Client::new("test".into());
            let cache = twilight_cache_inmemory::InMemoryCache::builder().build();
            let redis = new_pool();
            let client = mongodb::Client::with_uri_str("mongodb://localhost:27017")
                .await
                .unwrap();
            let db = client.database("test_db");
            let mongo = MongoDB {
                client,
                channels: db.collection::<Channel>("channels"),
                roles: db.collection::<Role>("roles"),
                quarantines: db.collection::<Quarantine>("quarantines"),
                messages: db.collection::<Message>("messages"),
                ai_prompts: db.collection::<AiPrompt>("ai_prompts"),
            };
            let reqwest = reqwest::Client::new();
            Arc::new(Context {
                http,
                cache,
                redis,
                mongo,
                reqwest,
            })
        })
        .await
        .clone()
    }

    #[tokio::test]
    async fn test_set_items_and_search() {
        let mut data: Vec<MarketEntry> = (0..30)
            .map(|i| MarketEntry {
                name: format!("Item{:02}", 29 - i),
                url: format!("url{i}"),
            })
            .collect();
        data.sort_unstable_by(|a, b| cmp_ignore_ascii_case(&a.name, &b.name));
        MarketService::set_items(data).await;
        let results = MarketService::search("").await;
        assert_eq!(results.len(), 25);
        let mut sorted = results.clone();
        sorted.sort_unstable_by(|a, b| cmp_ignore_ascii_case(a, b));
        assert_eq!(results, sorted);
    }

    #[tokio::test]
    async fn test_maybe_refresh_updates() {
        let ctx = build_context().await;
        LAST_UPDATE.store(0, Ordering::Relaxed);
        MarketService::maybe_refresh(&ctx).await;
        let last = LAST_UPDATE.load(Ordering::Relaxed);
        assert!(last > 0);
    }

    #[tokio::test]
    async fn test_find_url() {
        let entries = vec![
            MarketEntry {
                name: "Apple".into(),
                url: "apple".into(),
            },
            MarketEntry {
                name: "Banana".into(),
                url: "banana".into(),
            },
        ];
        MarketService::set_items(entries).await;
        assert_eq!(MarketService::find_url("Apple").await, Some("apple".into()));
        assert_eq!(MarketService::find_url("Unknown").await, None);
    }
}
