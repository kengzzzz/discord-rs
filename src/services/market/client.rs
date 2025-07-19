use std::{collections::BTreeMap, sync::atomic::AtomicU64};

use deadpool_redis::Pool;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{
    dbs::redis::{redis_get, redis_set},
    utils::ascii::cmp_ignore_ascii_case,
};

use crate::utils::http::HttpProvider;

use super::{MarketKind, cache::MarketEntry};

const ITEMS_URL: &str = "https://api.warframe.market/v1/items";
pub(super) const ITEM_URL: &str = "https://warframe.market/items/";

#[derive(Deserialize, Serialize)]
struct ItemsPayload {
    items: Vec<MarketItem>,
}

#[derive(Deserialize, Serialize)]
struct ItemsResponse {
    payload: ItemsPayload,
}

#[derive(Deserialize, Serialize)]
struct MarketItem {
    item_name: String,
    url_name: String,
}

#[derive(Deserialize, Serialize)]
struct OrdersPayload {
    orders: Vec<Order>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct OrdersResponse {
    payload: OrdersPayload,
}

#[derive(Deserialize, Serialize)]
pub(super) struct OrderUser {
    pub ingame_name: String,
    pub status: String,
}

#[derive(Deserialize, Serialize)]
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
        stored.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
        return Some(stored);
    }
    None
}

pub(super) async fn update_items<H>(
    client: &H,
    key: &str,
    items: &Lazy<RwLock<Vec<MarketEntry>>>,
    last_update: &Lazy<AtomicU64>,
    pool: &Pool,
) -> anyhow::Result<()>
where
    H: HttpProvider + Sync,
{
    let data: ItemsResponse = client.get_json(ITEMS_URL).await?;
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
    entries.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
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

pub(super) async fn fetch_orders<H>(client: &H, url: &str) -> anyhow::Result<Vec<Order>>
where
    H: HttpProvider + Sync,
{
    let data: OrdersResponse = client
        .get_json(&format!(
            "https://api.warframe.market/v1/items/{url}/orders"
        ))
        .await?;
    Ok(data.payload.orders)
}

pub(super) async fn fetch_orders_map<H>(
    client: &H,
    url: &str,
    kind: &MarketKind,
) -> anyhow::Result<(BTreeMap<u8, Vec<super::session::OrderInfo>>, Option<u8>)>
where
    H: HttpProvider + Sync,
{
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        context::Context,
        dbs::{mongo::MongoDB, redis::new_pool},
    };
    use once_cell::sync::Lazy;
    use tokio::sync::OnceCell;

    struct MockClient {
        items: ItemsResponse,
        orders: OrdersResponse,
    }

    #[async_trait::async_trait]
    impl HttpProvider for MockClient {
        async fn get_json<T>(&self, url: &str) -> anyhow::Result<T>
        where
            T: serde::de::DeserializeOwned + Send,
        {
            if url == ITEMS_URL {
                let v = serde_json::to_value(&self.items).unwrap();
                return Ok(serde_json::from_value(v).unwrap());
            }
            if url.starts_with("https://api.warframe.market/v1/items/") {
                let v = serde_json::to_value(&self.orders).unwrap();
                return Ok(serde_json::from_value(v).unwrap());
            }
            unreachable!()
        }

        fn as_reqwest(&self) -> &reqwest::Client {
            unimplemented!()
        }
    }

    async fn build_context() -> Arc<Context> {
        static CTX: OnceCell<Arc<Context>> = OnceCell::const_new();
        CTX.get_or_init(|| async {
            unsafe {
                std::env::set_var("REDIS_URL", "redis://127.0.0.1:6379");
            }
            let http = twilight_http::Client::new("test".into());
            let cache = twilight_cache_inmemory::InMemoryCache::builder().build();
            let redis = new_pool();
            let mongo = MongoDB::init(redis.clone(), false).await.unwrap();
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
    async fn test_load_from_redis() {
        let context = build_context().await;
        let key = "redis:test:load";
        let entries = vec![
            MarketEntry {
                name: "beta".into(),
                url: "beta".into(),
            },
            MarketEntry {
                name: "alpha".into(),
                url: "alpha".into(),
            },
            MarketEntry {
                name: "Alpha".into(),
                url: "alpha2".into(),
            },
        ];
        redis_set(&context.redis, key, &entries).await;

        let result = load_from_redis(&context.redis, key).await.unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "alpha");
        assert_eq!(result[1].name, "beta");
    }

    #[tokio::test]
    async fn test_update_items() {
        let context = build_context().await;
        static ITEMS: Lazy<RwLock<Vec<MarketEntry>>> = Lazy::new(|| RwLock::new(Vec::new()));
        static LAST: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

        let client = MockClient {
            items: ItemsResponse {
                payload: ItemsPayload {
                    items: vec![
                        MarketItem {
                            item_name: "Beta".into(),
                            url_name: "beta".into(),
                        },
                        MarketItem {
                            item_name: "alpha".into(),
                            url_name: "alpha".into(),
                        },
                        MarketItem {
                            item_name: "Alpha".into(),
                            url_name: "alpha2".into(),
                        },
                    ],
                },
            },
            orders: OrdersResponse {
                payload: OrdersPayload { orders: vec![] },
            },
        };

        update_items(&client, "redis:test:update", &ITEMS, &LAST, &context.redis)
            .await
            .unwrap();

        let guard = ITEMS.read().await;
        assert_eq!(guard.len(), 2);
        assert_eq!(guard[0].name, "alpha");
        assert_eq!(guard[1].name, "Beta");
        drop(guard);

        let stored = redis_get::<Vec<MarketEntry>>(&context.redis, "redis:test:update")
            .await
            .unwrap();
        assert_eq!(stored.len(), 2);
        assert!(LAST.load(std::sync::atomic::Ordering::Relaxed) > 0);
    }

    #[tokio::test]
    async fn test_fetch_orders_map() {
        let client = MockClient {
            items: ItemsResponse {
                payload: ItemsPayload { items: vec![] },
            },
            orders: OrdersResponse {
                payload: OrdersPayload {
                    orders: vec![
                        Order {
                            platinum: 10,
                            quantity: 1,
                            order_type: "sell".into(),
                            user: OrderUser {
                                ingame_name: "s1".into(),
                                status: "ingame".into(),
                            },
                            mod_rank: Some(2),
                        },
                        Order {
                            platinum: 12,
                            quantity: 1,
                            order_type: "sell".into(),
                            user: OrderUser {
                                ingame_name: "s2".into(),
                                status: "ingame".into(),
                            },
                            mod_rank: None,
                        },
                        Order {
                            platinum: 5,
                            quantity: 1,
                            order_type: "buy".into(),
                            user: OrderUser {
                                ingame_name: "b1".into(),
                                status: "ingame".into(),
                            },
                            mod_rank: Some(3),
                        },
                        Order {
                            platinum: 30,
                            quantity: 1,
                            order_type: "buy".into(),
                            user: OrderUser {
                                ingame_name: "b2".into(),
                                status: "ingame".into(),
                            },
                            mod_rank: Some(1),
                        },
                        Order {
                            platinum: 25,
                            quantity: 1,
                            order_type: "buy".into(),
                            user: OrderUser {
                                ingame_name: "b3".into(),
                                status: "ingame".into(),
                            },
                            mod_rank: Some(1),
                        },
                        Order {
                            platinum: 99,
                            quantity: 1,
                            order_type: "sell".into(),
                            user: OrderUser {
                                ingame_name: "off".into(),
                                status: "offline".into(),
                            },
                            mod_rank: Some(2),
                        },
                    ],
                },
            },
        };

        let (buy_map, buy_max) = fetch_orders_map(&client, "item", &MarketKind::Buy)
            .await
            .unwrap();
        assert_eq!(buy_max, Some(2));
        assert_eq!(buy_map[&2][0].platinum, 10);
        assert_eq!(buy_map[&0][0].platinum, 12);

        let (sell_map, sell_max) = fetch_orders_map(&client, "item", &MarketKind::Sell)
            .await
            .unwrap();
        assert_eq!(sell_max, Some(3));
        assert_eq!(sell_map[&1][0].platinum, 30);
        assert_eq!(sell_map[&1][1].platinum, 25);
    }
}
