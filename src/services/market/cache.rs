use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::context::Context;

use super::{MarketService, client};
use std::sync::Arc;

const REDIS_KEY: &str = "discord-bot:market-items";
const UPDATE_SECS: u16 = 60 * 60;

#[derive(Serialize, Deserialize)]
pub(super) struct StoredEntry {
    pub name: String,
    pub url: String,
}

pub(super) struct ItemEntry {
    pub name: String,
    pub url: String,
    pub lower: String,
}

static ITEMS: Lazy<RwLock<Vec<ItemEntry>>> = Lazy::new(|| RwLock::new(Vec::new()));
static LAST_UPDATE: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

impl MarketService {
    pub async fn init(ctx: Arc<Context>) {
        if let Some(data) = client::load_from_redis(&ctx.redis, REDIS_KEY).await {
            *ITEMS.write().await = data;
            LAST_UPDATE.store(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                Ordering::Relaxed,
            );
        } else if let Err(e) = client::update_items(
            ctx.reqwest.as_ref(),
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
        let p = prefix.to_lowercase();
        let items = ITEMS.read().await;
        items
            .iter()
            .filter(|item| item.lower.starts_with(&p))
            .take(25)
            .map(|item| item.name.clone())
            .collect()
    }

    async fn maybe_refresh(ctx: Arc<Context>) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_UPDATE.load(Ordering::Relaxed);
        if now.saturating_sub(last) > UPDATE_SECS as u64 {
            if let Err(e) = client::update_items(
                ctx.reqwest.as_ref(),
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
    }

    pub async fn search_with_update(ctx: Arc<Context>, prefix: &str) -> Vec<String> {
        let mut results = Self::search(prefix).await;
        if results.is_empty() {
            Self::maybe_refresh(ctx.clone()).await;
            results = Self::search(prefix).await;
        }
        results
    }

    pub(super) async fn find_url(name: &str) -> Option<String> {
        let lower = name.to_lowercase();
        let items = ITEMS.read().await;
        for item in items.iter() {
            if item.lower == lower {
                return Some(item.url.clone());
            }
        }
        None
    }
}
