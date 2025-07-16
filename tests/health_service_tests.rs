use axum::http::StatusCode;

use discord_bot::services::health::HealthService;

#[tokio::test]
async fn test_health_status() {
    HealthService::set_ready(false);
    HealthService::set_discord(false);
    HealthService::set_mongo(false);
    assert_eq!(
        HealthService::health().await,
        StatusCode::SERVICE_UNAVAILABLE
    );

    HealthService::set_ready(true);
    HealthService::set_discord(true);
    HealthService::set_mongo(true);
    assert_eq!(HealthService::health().await, StatusCode::OK);
}
