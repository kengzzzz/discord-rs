use crate::services::build::{self, BuildService};

#[tokio::test]
async fn test_build_sanitize_and_search() {
    build::BuildService::set_items(vec![
        ("Soma Prime".to_string(), "soma prime".to_string()),
        ("Serration".to_string(), "serration".to_string()),
    ]);

    let sanitized = BuildService::sanitize_item_name("Soma Prime & Burst");
    assert_eq!(sanitized, "soma-prime-%26-burst");

    let results = BuildService::search("se");
    assert_eq!(results, vec!["Serration".to_string()]);
}
