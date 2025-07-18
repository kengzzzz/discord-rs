#![allow(unused_imports)]
use httpmock::Method::GET;
use httpmock::MockServer;
use serde_json::json;

use discord_bot::warframe;
mod utils;
use utils::context::test_context;

#[tokio::test]
async fn test_title_and_time() {
    let title = warframe::utils::title_case("cetus day");
    assert_eq!(title, "**Cetus Day** ends");

    let formatted = warframe::utils::format_time("2025-01-01T00:00:00Z");
    assert_eq!(formatted, "<t:1735689600:R>");
}

#[tokio::test]
#[cfg(feature = "mock-redis")]
async fn test_steel_path_umbra() {
    let server = MockServer::start_async().await;
    warframe::api::tests::set_base_url(&server.url(""));

    let mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/steelPath");
            then.status(200).json_body(json!({
                "currentReward": {"name": "Umbra Forma Blueprint"},
                "expiry": "2025-01-01T00:00:00Z",
                "activation": chrono::Utc::now().to_rfc3339()
            }));
        })
        .await;

    let ctx = std::sync::Arc::new(test_context().await);
    let (field, is_umbra) = warframe::embed::steel_path_field(&ctx).await.unwrap();
    assert!(is_umbra);
    assert_eq!(
        field.name,
        format!(
            "{}Steel Path{}",
            discord_bot::configs::Reaction::Load.emoji(),
            discord_bot::configs::Reaction::Load.emoji()
        )
    );
    assert_eq!(
        field.value,
        "**Umbra Forma Blueprint**\nends <t:1735689600:R>"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn test_next_monday_duration() {
    use chrono::{DateTime, Datelike, Duration as ChronoDuration, Utc};
    let now = Utc::now();
    let weekday = now.weekday().number_from_monday();
    let days = if weekday == 1 { 7 } else { 8 - weekday } as i64;
    let next_day = now.date_naive() + ChronoDuration::days(days);
    let target = next_day.and_hms_opt(0, 0, 0).unwrap();
    let target_dt = DateTime::<Utc>::from_naive_utc_and_offset(target, Utc);
    let dur = target_dt - now;
    let dur = std::time::Duration::from_secs(dur.num_seconds() as u64);
    assert!(dur.as_secs() > 0);
    assert!(dur.as_secs() <= 8 * 24 * 60 * 60);
}
