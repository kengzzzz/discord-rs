use super::cache::ITEMS;
use crate::utils::ascii::cmp_ignore_ascii_case;

pub(crate) async fn set_items(mut items: Vec<String>) {
    items.sort_unstable_by(|a, b| cmp_ignore_ascii_case(a, b));
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
