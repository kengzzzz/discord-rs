use std::sync::Arc;

use super::*;
use crate::context::{Context, ContextBuilder, mock_http::MockClient as Client};

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
    let ctx = ContextBuilder::new()
        .http(Client::new())
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
}

#[tokio::test]
async fn test_load_from_redis() {
    let context = build_context().await;
    let key = "redis:test:load";
    let entries = vec![
        MarketEntry { name: "beta".into(), url: "beta".into() },
        MarketEntry { name: "alpha".into(), url: "alpha".into() },
        MarketEntry { name: "Alpha".into(), url: "alpha2".into() },
    ];
    redis_set(&context.redis, key, &entries).await;

    let result = load_from_redis(&context.redis, key)
        .await
        .unwrap();

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
                    MarketItem { item_name: "Beta".into(), url_name: "beta".into() },
                    MarketItem { item_name: "alpha".into(), url_name: "alpha".into() },
                    MarketItem { item_name: "Alpha".into(), url_name: "alpha2".into() },
                ],
            },
        },
        orders: OrdersResponse { payload: OrdersPayload { orders: vec![] } },
    };

    update_items(
        &client,
        "redis:test:update",
        &ITEMS,
        &LAST,
        &context.redis,
    )
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
        items: ItemsResponse { payload: ItemsPayload { items: vec![] } },
        orders: OrdersResponse {
            payload: OrdersPayload {
                orders: vec![
                    Order {
                        platinum: 10,
                        quantity: 1,
                        order_type: "sell".into(),
                        user: OrderUser { ingame_name: "s1".into(), status: "ingame".into() },
                        mod_rank: Some(2),
                    },
                    Order {
                        platinum: 12,
                        quantity: 1,
                        order_type: "sell".into(),
                        user: OrderUser { ingame_name: "s2".into(), status: "ingame".into() },
                        mod_rank: None,
                    },
                    Order {
                        platinum: 5,
                        quantity: 1,
                        order_type: "buy".into(),
                        user: OrderUser { ingame_name: "b1".into(), status: "ingame".into() },
                        mod_rank: Some(3),
                    },
                    Order {
                        platinum: 30,
                        quantity: 1,
                        order_type: "buy".into(),
                        user: OrderUser { ingame_name: "b2".into(), status: "ingame".into() },
                        mod_rank: Some(1),
                    },
                    Order {
                        platinum: 25,
                        quantity: 1,
                        order_type: "buy".into(),
                        user: OrderUser { ingame_name: "b3".into(), status: "ingame".into() },
                        mod_rank: Some(1),
                    },
                    Order {
                        platinum: 99,
                        quantity: 1,
                        order_type: "sell".into(),
                        user: OrderUser { ingame_name: "off".into(), status: "offline".into() },
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
