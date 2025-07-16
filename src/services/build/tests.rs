use super::cache::{ITEMS, ItemEntry};

pub(crate) async fn set_items(items: Vec<ItemEntry>) {
    *ITEMS.write().await = items;
}

#[tokio::test]
async fn test_build_sanitize_and_search() {
    set_items(vec![
        ("Soma Prime".to_string(), "soma prime".to_string()),
        ("Serration".to_string(), "serration".to_string()),
    ])
    .await;

    let sanitized = super::BuildService::sanitize_item_name("Soma Prime & Burst");
    assert_eq!(sanitized, "soma-prime-%26-burst");

    let results = super::BuildService::search("se").await;
    assert_eq!(results, vec!["Serration".to_string()]);
}
