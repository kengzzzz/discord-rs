use std::{
    collections::HashSet,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use once_cell::sync::Lazy;
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::{
    context::Context,
    dbs::redis::{redis_get, redis_set},
    services::http::HttpService,
};

use reqwest::Client;
use std::sync::Arc;

const ITEMS_URL: &str =
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/All.json";
const REDIS_KEY: &str = "discord-bot:build-items";
const UPDATE_SECS: u16 = 60 * 60;

pub(crate) type ItemEntry = (String, String); // (original, lowercase)
pub(crate) static ITEMS: Lazy<RwLock<Vec<ItemEntry>>> = Lazy::new(|| RwLock::new(Vec::new()));
pub(crate) static LAST_UPDATE: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

#[derive(Deserialize)]
struct Item {
    name: String,
    category: String,
    #[serde(rename = "productCategory")]
    product_category: Option<String>,
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

async fn load_from_redis() -> Option<Vec<ItemEntry>> {
    if let Some(names) = redis_get::<Vec<String>>(REDIS_KEY).await {
        let entries = names
            .into_iter()
            .map(|n| {
                let lower = n.to_lowercase();
                (n, lower)
            })
            .collect();
        return Some(entries);
    }
    None
}

async fn update_items(client: &Client) -> anyhow::Result<()> {
    let resp = HttpService::get(client, ITEMS_URL).await?;
    let fetched: Vec<Item> = resp.json().await?;
    let mut set = HashSet::new();
    let mut names = Vec::new();
    let mut original = Vec::new();
    for item in fetched {
        if filter(&item) {
            let lower = item.name.to_lowercase();
            if set.insert(lower.clone()) {
                original.push(item.name.clone());
                names.push((item.name, lower));
            }
        }
    }
    names.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    original.sort_unstable();
    *ITEMS.write().await = names;
    redis_set(REDIS_KEY, &original).await;
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
        if let Some(data) = load_from_redis().await {
            *ITEMS.write().await = data;
            LAST_UPDATE.store(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                Ordering::Relaxed,
            );
        } else if let Err(e) = update_items(ctx.reqwest.as_ref()).await {
            tracing::warn!(error = %e, "failed to update build items");
        }
    }

    pub async fn search(prefix: &str) -> Vec<String> {
        let p = prefix.to_lowercase();
        let items = ITEMS.read().await;
        items
            .iter()
            .filter(|(_, lower)| lower.starts_with(&p))
            .take(25)
            .map(|(orig, _)| orig.clone())
            .collect()
    }

    async fn maybe_refresh(client: &Client) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_UPDATE.load(Ordering::Relaxed);
        if now.saturating_sub(last) > UPDATE_SECS as u64 {
            if let Err(e) = update_items(client).await {
                tracing::warn!(error = %e, "failed to update build items");
            }
        }
    }

    pub async fn search_with_update(client: &Client, prefix: &str) -> Vec<String> {
        let mut results = Self::search(prefix).await;
        if results.is_empty() {
            Self::maybe_refresh(client).await;
            results = Self::search(prefix).await;
        }
        results
    }
}
