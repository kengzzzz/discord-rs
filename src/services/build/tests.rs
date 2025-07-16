#![allow(clippy::unnecessary_sort_by)]

use super::cache::ITEMS;

pub(crate) async fn set_items(mut items: Vec<String>) {
    items.sort_unstable_by_key(|n| n.to_ascii_lowercase());
    *ITEMS.write().await = items;
}

#[tokio::test]
async fn test_build_sanitize_and_search() {
    set_items(vec!["Soma Prime".to_string(), "Serration".to_string()]).await;

    let sanitized = super::BuildService::sanitize_item_name("Soma Prime & Burst");
    assert_eq!(sanitized, "soma-prime-%26-burst");

    let results = super::BuildService::search("se").await;
    assert_eq!(results, vec!["Serration".to_string()]);
}
