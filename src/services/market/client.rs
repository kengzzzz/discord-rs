use std::{collections::BTreeMap, sync::atomic::AtomicU64};

use deadpool_redis::Pool;
use once_cell::sync::Lazy;
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::{
    dbs::redis::{redis_get, redis_set},
    services::http::HttpService,
    utils::ascii::{ascii_eq_ignore_case, cmp_ignore_ascii_case},
};

use reqwest::Client;

use super::{MarketKind, cache::MarketEntry};

const ITEMS_URL: &str = "https://api.warframe.market/v1/items";
pub(super) const ITEM_URL: &str = "https://warframe.market/items/";

#[derive(Deserialize)]
struct ItemsPayload {
    items: Vec<MarketItem>,
}

#[derive(Deserialize)]
struct ItemsResponse {
    payload: ItemsPayload,
}

#[derive(Deserialize)]
struct MarketItem {
    item_name: String,
    url_name: String,
}

#[derive(Deserialize)]
struct OrdersPayload {
    orders: Vec<Order>,
}

#[derive(Deserialize)]
pub(super) struct OrdersResponse {
    payload: OrdersPayload,
}

#[derive(Deserialize)]
pub(super) struct OrderUser {
    pub ingame_name: String,
    pub status: String,
}

#[derive(Deserialize)]
pub(super) struct Order {
    pub platinum: u32,
    pub quantity: u32,
    pub order_type: String,
    pub user: OrderUser,
    #[serde(default)]
    pub mod_rank: Option<u8>,
}

pub(super) async fn load_from_redis(pool: &Pool, key: &str) -> Option<Vec<MarketEntry>> {
    if let Some(mut stored) = redis_get::<Vec<MarketEntry>>(pool, key).await {
        stored.sort_unstable_by(|a, b| cmp_ignore_ascii_case(&a.name, &b.name));
        stored.dedup_by(|a, b| ascii_eq_ignore_case(&a.name, &b.name));
        return Some(stored);
    }
    None
}

pub(super) async fn update_items(
    client: &Client,
    key: &str,
    items: &Lazy<RwLock<Vec<MarketEntry>>>,
    last_update: &Lazy<AtomicU64>,
    pool: &Pool,
) -> anyhow::Result<()> {
    let resp = HttpService::get(client, ITEMS_URL).await?;
    let data: ItemsResponse = resp.json().await?;
    let mut entries: Vec<MarketEntry> = data
        .payload
        .items
        .into_iter()
        .map(|item| MarketEntry {
            name: item.item_name,
            url: item.url_name,
        })
        .collect();
    entries.sort_unstable_by(|a, b| cmp_ignore_ascii_case(&a.name, &b.name));
    entries.dedup_by(|a, b| ascii_eq_ignore_case(&a.name, &b.name));
    redis_set(pool, key, &entries).await;
    let mut guard = items.write().await;
    *guard = entries;
    last_update.store(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        std::sync::atomic::Ordering::Relaxed,
    );
    Ok(())
}

pub(super) async fn fetch_orders(client: &Client, url: &str) -> anyhow::Result<Vec<Order>> {
    let resp = HttpService::get(
        client,
        format!("https://api.warframe.market/v1/items/{url}/orders"),
    )
    .await?;
    let data: OrdersResponse = resp.json().await?;
    Ok(data.payload.orders)
}

pub(super) async fn fetch_orders_map(
    client: &Client,
    url: &str,
    kind: &MarketKind,
) -> anyhow::Result<(BTreeMap<u8, Vec<super::session::OrderInfo>>, Option<u8>)> {
    let orders = fetch_orders(client, url).await?;
    let mut by_rank: BTreeMap<u8, Vec<super::session::OrderInfo>> = BTreeMap::new();
    let mut max_rank: Option<u8> = None;
    for o in orders {
        if o.user.status != "ingame" || o.order_type == kind.action() {
            continue;
        }
        let rank = o.mod_rank.unwrap_or(0);
        if let Some(m) = max_rank {
            if rank > m {
                max_rank = Some(rank);
            }
        } else if o.mod_rank.is_some() {
            max_rank = Some(rank);
        }
        by_rank
            .entry(rank)
            .or_default()
            .push(super::session::OrderInfo {
                quantity: o.quantity,
                platinum: o.platinum,
                ign: o.user.ingame_name,
            });
    }
    for vec in by_rank.values_mut() {
        if kind.target_type() == "sell" {
            vec.sort_unstable_by_key(|o| o.platinum);
        } else {
            vec.sort_unstable_by(|a, b| b.platinum.cmp(&a.platinum));
        }
    }
    Ok((by_rank, max_rank))
}
