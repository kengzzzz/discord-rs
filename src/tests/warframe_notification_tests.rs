use httpmock::Method::GET;
use httpmock::MockServer;
use serde_json::json;

use crate::warframe;

#[tokio::test]
async fn test_title_and_time() {
    let title = warframe::title_case("cetus day");
    assert_eq!(title, "**Cetus **Day** ends");

    let formatted = warframe::format_time("2025-01-01T00:00:00Z");
    assert_eq!(formatted, "<t:1735689600:R>");
}

#[tokio::test]
async fn test_steel_path_umbra() {
    let server = MockServer::start_async().await;
    crate::warframe::api::set_base_url(&server.url(""));

    server
        .mock_async(|when, then| {
            when.method(GET).path("/steelPath");
            then.status(200).json_body(json!({
                "currentReward": {"name": "Umbra Forma Blueprint"},
                "expiry": "2025-01-01T00:00:00Z",
                "activation": chrono::Utc::now().to_rfc3339()
            }));
        })
        .await;

    let ctx = std::sync::Arc::new(crate::context::Context::test().await);
    let (field, is_umbra) = warframe::steel_path_field(ctx).await.unwrap();
    assert!(is_umbra);
    assert_eq!(
        field.name,
        format!(
            "{}Steel Path{}",
            crate::configs::Reaction::Load.emoji(),
            crate::configs::Reaction::Load.emoji()
        )
    );
    assert_eq!(
        field.value,
        "**Umbra Forma Blueprint**\nends <t:1735689600:R>"
    );
}

#[tokio::test]
async fn test_next_monday_duration() {
    let dur = crate::services::notification::next_monday_duration();
    assert!(dur.as_secs() > 0);
    assert!(dur.as_secs() <= 8 * 24 * 60 * 60);
}
