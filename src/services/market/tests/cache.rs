use super::*;
use crate::context::{Context, ContextBuilder, mock_http::MockClient as Client};

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
async fn test_set_items_and_search() {
    let mut data: Vec<MarketEntry> = (0..30)
        .map(|i| MarketEntry {
            name: format!("Item{:02}", 29 - i),
            item_id: format!("id{i}"),
            slug: format!("slug{i}"),
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
    ctx.reqwest.add_json_response(
        "https://api.warframe.market/v2/items",
        "{ \"data\": [] }",
    );
    LAST_UPDATE.store(0, Ordering::Relaxed);
    MarketService::maybe_refresh(&ctx).await;
    let last = LAST_UPDATE.load(Ordering::Relaxed);
    assert!(last > 0);
}

#[tokio::test]
async fn test_find_item() {
    let entries = vec![
        MarketEntry { name: "Apple".into(), item_id: "apple-id".into(), slug: "apple".into() },
        MarketEntry { name: "Banana".into(), item_id: "banana-id".into(), slug: "banana".into() },
    ];
    MarketService::set_items(entries).await;
    assert_eq!(
        MarketService::find_item("Apple")
            .await
            .map(|item| (item.item_id, item.slug)),
        Some(("apple-id".into(), "apple".into()))
    );
    assert!(
        MarketService::find_item("Unknown")
            .await
            .is_none()
    );
}
