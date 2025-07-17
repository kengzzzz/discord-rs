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

    async fn maybe_refresh(ctx: Arc<Context>) {
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

    pub async fn search_with_update(ctx: Arc<Context>, prefix: &str) -> Vec<String> {
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
