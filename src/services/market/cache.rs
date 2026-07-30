use std::{
    cmp,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

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
    #[serde(default)]
    pub item_id: String,
    #[serde(default, alias = "url")]
    pub slug: String,
}

static ITEMS: Lazy<RwLock<Vec<MarketEntry>>> = Lazy::new(|| RwLock::new(Vec::new()));
static LAST_UPDATE: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
static REFRESH_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

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
        } else if let Err(e) = client::update_items(
            &ctx.reqwest,
            REDIS_KEY,
            &ITEMS,
            &LAST_UPDATE,
            &ctx.redis,
        )
        .await
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

    fn cache_is_stale() -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_UPDATE.load(Ordering::Relaxed);
        now.saturating_sub(last) > UPDATE_SECS as u64
    }

    async fn maybe_refresh(ctx: &Arc<Context>) {
        if Self::cache_is_stale()
            && let Err(e) = client::update_items(
                &ctx.reqwest,
                REDIS_KEY,
                &ITEMS,
                &LAST_UPDATE,
                &ctx.redis,
            )
            .await
        {
            tracing::warn!(error = %e, "failed to update market items");
        }
    }

    fn refresh_in_background(ctx: &Arc<Context>) {
        if !Self::cache_is_stale() {
            return;
        }
        let Ok(guard) = REFRESH_LOCK.try_lock() else {
            return;
        };

        let ctx = Arc::clone(ctx);
        tokio::spawn(async move {
            let _guard = guard;
            Self::maybe_refresh(&ctx).await;
        });
    }

    pub async fn search_with_update(ctx: &Arc<Context>, prefix: &str) -> Vec<String> {
        let results = Self::search(prefix).await;
        Self::refresh_in_background(ctx);
        results
    }

    pub(super) async fn find_item(name: &str) -> Option<MarketEntry> {
        let items = ITEMS.read().await;
        let idx =
            items.partition_point(|e| cmp_ignore_ascii_case(&e.name, name) == cmp::Ordering::Less);
        if idx < items.len()
            && cmp_ignore_ascii_case(&items[idx].name, name) == cmp::Ordering::Equal
            && !items[idx].item_id.is_empty()
            && !items[idx].slug.is_empty()
        {
            Some(items[idx].clone())
        } else {
            None
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[allow(dead_code)]
#[path = "tests/cache.rs"]
mod tests;
