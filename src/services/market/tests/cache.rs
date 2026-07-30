use super::*;
use crate::context::{Context, ContextBuilder, mock_http::MockClient as Client};

// ITEMS and LAST_UPDATE are process-global; serialize the tests that mutate them.
static MARKET_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn build_context() -> Arc<Context> {
    let ctx = ContextBuilder::new()
        .http(Client::new())
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
}

async fn wait_for_requests(ctx: &Context, expected: usize) {
    tokio::time::timeout(std::time::Duration::from_millis(100), async {
        while ctx
            .reqwest
            .json_request_count("https://api.warframe.market/v2/items")
            < expected
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("background refresh did not start");
}

async fn wait_for_refresh_completion() {
    tokio::time::timeout(std::time::Duration::from_millis(250), async {
        loop {
            if let Ok(guard) = REFRESH_LOCK.try_lock() {
                drop(guard);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("background refresh did not complete");
}

#[tokio::test]
async fn test_set_items_and_search() {
    let _guard = MARKET_LOCK.lock().await;
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
    let _guard = MARKET_LOCK.lock().await;
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
async fn stale_search_does_not_wait_for_refresh() {
    let _guard = MARKET_LOCK.lock().await;
    let ctx = build_context().await;
    MarketService::set_items(vec![MarketEntry {
        name: "Cached Item".into(),
        item_id: "cached-id".into(),
        slug: "cached-item".into(),
    }])
    .await;
    ctx.reqwest.add_json_response(
        "https://api.warframe.market/v2/items",
        "{ \"data\": [] }",
    );
    ctx.reqwest.set_json_delay(
        "https://api.warframe.market/v2/items",
        std::time::Duration::from_millis(100),
    );
    LAST_UPDATE.store(0, Ordering::Relaxed);

    let results = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        MarketService::search_with_update(&ctx, "Missing"),
    )
    .await
    .expect("autocomplete search blocked while refreshing its cache");

    assert!(results.is_empty());
    wait_for_requests(&ctx, 1).await;
    wait_for_refresh_completion().await;
}

#[tokio::test]
async fn fresh_search_does_not_request_refresh() {
    let _guard = MARKET_LOCK.lock().await;
    let ctx = build_context().await;
    MarketService::set_items(vec![MarketEntry {
        name: "Cached Item".into(),
        item_id: "cached-id".into(),
        slug: "cached-item".into(),
    }])
    .await;
    LAST_UPDATE.store(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        Ordering::Relaxed,
    );

    let results = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        MarketService::search_with_update(&ctx, "Missing"),
    )
    .await
    .expect("fresh autocomplete search should not refresh its cache");

    assert!(results.is_empty());
    assert_eq!(
        ctx.reqwest
            .json_request_count("https://api.warframe.market/v2/items"),
        0
    );
}

#[tokio::test]
async fn concurrent_stale_searches_start_one_refresh() {
    let _guard = MARKET_LOCK.lock().await;
    let ctx = build_context().await;
    MarketService::set_items(vec![MarketEntry {
        name: "Cached Item".into(),
        item_id: "cached-id".into(),
        slug: "cached-item".into(),
    }])
    .await;
    ctx.reqwest.add_json_response(
        "https://api.warframe.market/v2/items",
        "{ \"data\": [] }",
    );
    ctx.reqwest.set_json_delay(
        "https://api.warframe.market/v2/items",
        std::time::Duration::from_millis(100),
    );
    LAST_UPDATE.store(0, Ordering::Relaxed);

    MarketService::search_with_update(&ctx, "Missing").await;
    wait_for_requests(&ctx, 1).await;
    for _ in 0..5 {
        MarketService::search_with_update(&ctx, "Missing").await;
    }

    assert_eq!(
        ctx.reqwest
            .json_request_count("https://api.warframe.market/v2/items"),
        1
    );
    wait_for_refresh_completion().await;
}

#[tokio::test]
async fn cached_matches_survive_refresh_failure() {
    let _guard = MARKET_LOCK.lock().await;
    let ctx = build_context().await;
    MarketService::set_items(vec![MarketEntry {
        name: "Cached Item".into(),
        item_id: "cached-id".into(),
        slug: "cached-item".into(),
    }])
    .await;
    ctx.reqwest
        .add_json_response("https://api.warframe.market/v2/items", "not-json");
    LAST_UPDATE.store(0, Ordering::Relaxed);

    let results = MarketService::search_with_update(&ctx, "Cached").await;
    wait_for_requests(&ctx, 1).await;
    wait_for_refresh_completion().await;

    assert_eq!(results, ["Cached Item"]);
    assert_eq!(
        MarketService::search("Cached").await,
        ["Cached Item"]
    );
}

#[tokio::test]
async fn test_find_item() {
    let _guard = MARKET_LOCK.lock().await;
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
