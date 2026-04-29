use std::time::Duration;

use super::*;

#[tokio::test]
async fn admits_immediately_when_capacity_is_available() {
    let scheduler = AiScheduler::with_window(Duration::from_millis(50));
    let _guard = scheduler
        .acquire(
            "gemini-2.5-flash",
            AiOperation::Chat,
            AdmissionConfig { rpm_limit: 2, queue_timeout: Duration::from_millis(20) },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn times_out_when_queue_wait_exceeds_deadline() {
    let scheduler = AiScheduler::with_window(Duration::from_millis(40));
    let _guard = scheduler
        .acquire(
            "gemini-2.5-flash",
            AiOperation::Chat,
            AdmissionConfig { rpm_limit: 1, queue_timeout: Duration::from_millis(10) },
        )
        .await
        .unwrap();

    let result = scheduler
        .acquire(
            "gemini-2.5-flash",
            AiOperation::Chat,
            AdmissionConfig { rpm_limit: 1, queue_timeout: Duration::from_millis(10) },
        )
        .await;

    match result {
        Ok(_) => panic!("second request should time out"),
        Err(err) => assert!(
            err.to_string()
                .contains("ai scheduler queue timeout")
        ),
    }
}

#[tokio::test]
async fn cooldown_blocks_until_it_expires() {
    let scheduler = AiScheduler::with_window(Duration::from_millis(10));
    scheduler
        .cool_down(
            "gemini-2.5-flash",
            AiOperation::Chat,
            Duration::from_millis(25),
        )
        .await;

    let start = std::time::Instant::now();
    let _guard = scheduler
        .acquire(
            "gemini-2.5-flash",
            AiOperation::Chat,
            AdmissionConfig { rpm_limit: 5, queue_timeout: Duration::from_millis(60) },
        )
        .await
        .unwrap();

    assert!(start.elapsed() >= Duration::from_millis(20));
}
