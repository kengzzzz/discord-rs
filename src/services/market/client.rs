use std::{collections::BTreeMap, sync::atomic::AtomicU64};

use deadpool_redis::Pool;
use once_cell::sync::Lazy;
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::{
    dbs::redis::{redis_get, redis_set},
    services::http::HttpService,
};

use reqwest::Client;

use super::{
    MarketKind,
    cache::{ItemEntry, StoredEntry},
};

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

pub(super) async fn load_from_redis(pool: &Pool, key: &str) -> Option<BTreeMap<String, ItemEntry>> {
    if let Some(stored) = redis_get::<Vec<StoredEntry>>(pool, key).await {
        let mut map = BTreeMap::new();
        for s in stored {
            map.insert(
                s.name.to_lowercase(),
                ItemEntry {
                    name: s.name,
                    url: s.url,
                },
            );
        }
        return Some(map);
    }
    None
}

pub(super) async fn update_items(
    client: &Client,
    key: &str,
    items: &Lazy<RwLock<BTreeMap<String, ItemEntry>>>,
    last_update: &Lazy<AtomicU64>,
    pool: &Pool,
) -> anyhow::Result<()> {
    let resp = HttpService::get(client, ITEMS_URL).await?;
    let data: ItemsResponse = resp.json().await?;
    let mut stored = Vec::new();
    let mut new_items = BTreeMap::new();
    for item in data.payload.items {
        stored.push(StoredEntry {
            name: item.item_name.clone(),
            url: item.url_name.clone(),
        });
        new_items.insert(
            item.item_name.to_lowercase(),
            ItemEntry {
                name: item.item_name,
                url: item.url_name,
            },
        );
    }
    stored.sort_by(|a, b| a.name.cmp(&b.name));
    redis_set(pool, key, &stored).await;
    *items.write().await = new_items;
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
            vec.sort_by_key(|o| o.platinum);
        } else {
            vec.sort_by(|a, b| b.platinum.cmp(&a.platinum));
        }
    }
    Ok((by_rank, max_rank))
}
