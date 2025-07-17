use super::build_cache_utils::ITEMS_URL_OVERRIDE as BUILD_URL;
use crate::context::Context;
use crate::services::build::cache::{ITEMS, update_items as update_build_items};
use httpmock::{Method::GET, MockServer};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn test_build_items_not_modified() {
    let server = MockServer::start_async().await;
    BUILD_URL.set(server.url("/items")).ok();
    let data = json!([
        {"name": "Item1", "category": "Primary", "productCategory": null},
        {"name": "Item2", "category": "Melee", "productCategory": null}
    ]);
    server
        .mock_async(|when, then| {
            when.method(GET).path("/items");
            then.status(200)
                .header("ETag", "v1")
                .json_body(data.clone());
        })
        .await;
    let ctx = Arc::new(Context::test().await);
    update_build_items(&ctx.reqwest, &ctx.redis).await.unwrap();
    let first_len = ITEMS.read().await.len();
    assert_eq!(first_len, 2);
    // second request with 304
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/items")
                .header("if-none-match", "v1");
            then.status(304);
        })
        .await;
    update_build_items(&ctx.reqwest, &ctx.redis).await.unwrap();
    let second_len = ITEMS.read().await.len();
    assert_eq!(second_len, first_len);
}
