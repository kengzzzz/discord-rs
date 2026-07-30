use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering as AtomicOrdering},
};
use std::time::Duration;

// Build cache state is process-global; serialize tests that mutate it.
static BUILD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn mark_cache_fresh() {
    LAST_UPDATE.store(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        Ordering::Relaxed,
    );
}

async fn wait_for_refreshes(refreshes: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_millis(100), async {
        while refreshes.load(AtomicOrdering::SeqCst) < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("background refresh did not start");
}

async fn wait_for_refresh_completion() {
    tokio::time::timeout(Duration::from_millis(250), async {
        loop {
            if let Ok(guard) = REFRESH_LOCK.try_lock() {
                drop(guard);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("background refresh did not complete");
}

#[tokio::test]
async fn stale_search_does_not_wait_for_refresh() {
    let _guard = BUILD_LOCK.lock().await;
    *ITEMS.write().await = vec!["Cached Item".into()];
    LAST_UPDATE.store(0, Ordering::Relaxed);
    let refreshes = Arc::new(AtomicUsize::new(0));
    let refresh_count = Arc::clone(&refreshes);

    let results = tokio::time::timeout(
        Duration::from_millis(20),
        BuildService::search_with_refresh("Cached", move || async move {
            refresh_count.fetch_add(1, AtomicOrdering::SeqCst);
            tokio::time::sleep(Duration::from_millis(100)).await;
            mark_cache_fresh();
            Ok(())
        }),
    )
    .await
    .expect("autocomplete search blocked while refreshing its cache");

    assert_eq!(results, ["Cached Item"]);
    wait_for_refreshes(&refreshes, 1).await;
    wait_for_refresh_completion().await;
}

#[tokio::test]
async fn concurrent_stale_searches_start_one_refresh() {
    let _guard = BUILD_LOCK.lock().await;
    *ITEMS.write().await = vec!["Cached Item".into()];
    LAST_UPDATE.store(0, Ordering::Relaxed);
    let refreshes = Arc::new(AtomicUsize::new(0));
    let first_count = Arc::clone(&refreshes);

    BuildService::search_with_refresh("Missing", move || async move {
        first_count.fetch_add(1, AtomicOrdering::SeqCst);
        tokio::time::sleep(Duration::from_millis(100)).await;
        mark_cache_fresh();
        Ok(())
    })
    .await;
    wait_for_refreshes(&refreshes, 1).await;

    for _ in 0..5 {
        let refresh_count = Arc::clone(&refreshes);
        BuildService::search_with_refresh("Missing", move || async move {
            refresh_count.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        })
        .await;
    }

    assert_eq!(refreshes.load(AtomicOrdering::SeqCst), 1);
    wait_for_refresh_completion().await;
}

#[tokio::test]
async fn cached_matches_survive_refresh_failure() {
    let _guard = BUILD_LOCK.lock().await;
    *ITEMS.write().await = vec!["Cached Item".into()];
    LAST_UPDATE.store(0, Ordering::Relaxed);
    let refreshes = Arc::new(AtomicUsize::new(0));
    let refresh_count = Arc::clone(&refreshes);

    let results = BuildService::search_with_refresh("Cached", move || async move {
        refresh_count.fetch_add(1, AtomicOrdering::SeqCst);
        anyhow::bail!("provider unavailable")
    })
    .await;
    wait_for_refreshes(&refreshes, 1).await;
    wait_for_refresh_completion().await;

    assert_eq!(results, ["Cached Item"]);
    assert_eq!(
        BuildService::search("Cached").await,
        ["Cached Item"]
    );
}
