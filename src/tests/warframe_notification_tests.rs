use httpmock::Method::GET;
use httpmock::MockServer;
use serde_json::json;

use crate::services::{notification::NotificationService, warframe};

#[tokio::test]
async fn test_title_and_time() {
    let title = warframe::title_case_test("cetus day");
    assert_eq!(title, "**Cetus **Day** ends");

    let formatted = warframe::format_time_test("2025-01-01T00:00:00Z");
    assert_eq!(formatted, "<t:1735689600:R>");
}

#[tokio::test]
async fn test_steel_path_umbra() {
    let server = MockServer::start_async().await;
    warframe::set_base_url(&server.url(""));

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

    let (field, is_umbra) = warframe::steel_path_field_test().await.unwrap();
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
    let dur = NotificationService::next_monday_duration_test();
    assert!(dur.as_secs() > 0);
    assert!(dur.as_secs() <= 8 * 24 * 60 * 60);
}
