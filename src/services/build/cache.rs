use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
use crate::tests::build_cache_utils::ITEMS_URL_OVERRIDE;
use deadpool_redis::Pool;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{
    context::Context,
    dbs::redis::{redis_get, redis_set},
    services::http::HttpService,
    utils::comparator::{ascii_eq_ignore_case, cmp_ignore_ascii_case, collect_prefix_icase},
};

use reqwest::Client;
use std::sync::Arc;

const ITEMS_URL: &str =
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/All.json";
const REDIS_KEY: &str = "discord-bot:build-items";
const UPDATE_SECS: u16 = 60 * 60;

pub(crate) static ITEMS: Lazy<RwLock<Vec<String>>> = Lazy::new(|| RwLock::new(Vec::new()));
pub(crate) static LAST_UPDATE: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
pub(crate) static ITEMS_ETAG: Lazy<RwLock<Option<String>>> = Lazy::new(|| RwLock::new(None));

#[derive(Deserialize)]
struct Item {
    name: String,
    category: String,
    #[serde(rename = "productCategory")]
    product_category: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct StoredItems {
    names: Vec<String>,
    etag: Option<String>,
}

const CATEGORY: [&str; 9] = [
    "Primary",
    "Melee",
    "Secondary",
    "Pets",
    "Arch-Melee",
    "Archwing",
    "Warframes",
    "Sentinels",
    "Arch-Gun",
];

fn items_url() -> String {
    #[cfg(test)]
    {
        if let Some(u) = ITEMS_URL_OVERRIDE.get() {
            return u.clone();
        }
    }
    ITEMS_URL.to_string()
}

fn filter(item: &Item) -> bool {
    if CATEGORY.contains(&item.category.as_str()) {
        return true;
    }
    if item.category == "Misc" {
        if let Some(pc) = &item.product_category {
            return pc == "Pistols" || pc == "SpecialItems";
        }
    }
    false
}

async fn load_from_redis(pool: &Pool) -> Option<Vec<String>> {
    if let Some(stored) = redis_get::<StoredItems>(pool, REDIS_KEY).await {
        *ITEMS_ETAG.write().await = stored.etag;
        let mut names = stored.names;
        names.sort_unstable_by(|a, b| cmp_ignore_ascii_case(a, b));
        names.dedup_by(|a, b| ascii_eq_ignore_case(a, b));
        return Some(names);
    }
    None
}

pub(crate) async fn update_items(client: &Client, pool: &Pool) -> anyhow::Result<()> {
    use reqwest::header::{ETAG, HeaderMap, HeaderValue, IF_NONE_MATCH};
    let mut headers = HeaderMap::new();
    if let Some(tag) = &*ITEMS_ETAG.read().await {
        if let Ok(v) = HeaderValue::from_str(tag) {
            headers.insert(IF_NONE_MATCH, v);
        }
    }
    let resp = HttpService::get_with_headers(client, items_url(), headers).await?;
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        LAST_UPDATE.store(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            Ordering::Relaxed,
        );
        return Ok(());
    }
    let new_etag = resp
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let fetched: Vec<Item> = resp.json().await?;
    let mut names: Vec<String> = fetched
        .into_iter()
        .filter(filter)
        .map(|item| item.name)
        .collect();
    names.sort_unstable_by(|a, b| cmp_ignore_ascii_case(a, b));
    names.dedup_by(|a, b| ascii_eq_ignore_case(a, b));
    *ITEMS.write().await = names.clone();
    redis_set(
        pool,
        REDIS_KEY,
        &StoredItems {
            names,
            etag: new_etag.clone(),
        },
    )
    .await;
    *ITEMS_ETAG.write().await = new_etag;
    LAST_UPDATE.store(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        Ordering::Relaxed,
    );
    Ok(())
}

use super::BuildService;

impl BuildService {
    pub async fn init(ctx: Arc<Context>) {
        if let Some(data) = load_from_redis(&ctx.redis).await {
            *ITEMS.write().await = data;
            LAST_UPDATE.store(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                Ordering::Relaxed,
            );
        } else if let Err(e) = update_items(ctx.reqwest.as_ref(), &ctx.redis).await {
            tracing::warn!(error = %e, "failed to update build items");
        }
    }

    pub async fn search(prefix: &str) -> Vec<String> {
        let items = ITEMS.read().await;
        if items.is_empty() {
            return Vec::new();
        }
        collect_prefix_icase(&items, prefix, |s| s)
    }

    async fn maybe_refresh(client: &Client, pool: &Pool) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_UPDATE.load(Ordering::Relaxed);
        if now.saturating_sub(last) > UPDATE_SECS as u64 {
            if let Err(e) = update_items(client, pool).await {
                tracing::warn!(error = %e, "failed to update build items");
            }
        }
    }

    pub async fn search_with_update(client: &Client, pool: &Pool, prefix: &str) -> Vec<String> {
        let mut results = Self::search(prefix).await;
        if results.is_empty() {
            Self::maybe_refresh(client, pool).await;
            results = Self::search(prefix).await;
        }
        results
    }
}
