use mongodb::bson::Document;
use mongodb::change_stream::event::ChangeStreamEvent;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use crate::{
    services::notification::notify_loop_mock,
    tests::{
        mock_db::{init_mock, spawn_watcher_mock},
        mock_http::MockHttp,
    },
};

#[tokio::test]
async fn test_watcher_reconnect_and_shutdown() {
    let map = init_mock();
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let token = CancellationToken::new();
    let calls = std::sync::Arc::new(tokio::sync::Mutex::new(0u32));
    let calls_clone = calls.clone();
    spawn_watcher_mock(
        "reconnect",
        ReceiverStream::new(rx),
        move |_| {
            let calls = calls_clone.clone();
            async move {
                let mut lock = calls.lock().await;
                *lock += 1;
            }
        },
        token.clone(),
    )
    .await
    .unwrap();

    tx.send(Err(std::io::Error::other("err").into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(15)).await;
    let evt_json = "{\"_id\":{\"_data\":\"token2\"},\"operationType\":\"insert\"}";
    let evt: ChangeStreamEvent<Document> = serde_json::from_str(evt_json).unwrap();
    let _ = tx.send(Ok(evt)).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    token.cancel();
    tokio::time::sleep(Duration::from_millis(20)).await;

    assert_eq!(*calls.lock().await, 1);
    assert!(
        map.lock()
            .await
            .get("changestream:resume:reconnect")
            .is_some()
    );
}

#[tokio::test]
async fn test_watcher_ignores_after_shutdown() {
    let map = init_mock();
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let token = CancellationToken::new();
    let calls = std::sync::Arc::new(tokio::sync::Mutex::new(0u32));
    let calls_clone = calls.clone();
    spawn_watcher_mock(
        "shutdown",
        ReceiverStream::new(rx),
        move |_| {
            let calls = calls_clone.clone();
            async move {
                let mut lock = calls.lock().await;
                *lock += 1;
            }
        },
        token.clone(),
    )
    .await
    .unwrap();

    let evt_json = "{\"_id\":{\"_data\":\"token1\"},\"operationType\":\"insert\"}";
    let evt: ChangeStreamEvent<Document> = serde_json::from_str(evt_json).unwrap();
    tx.send(Ok(evt)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    token.cancel();
    tokio::time::sleep(Duration::from_millis(10)).await;
    let evt_json = "{\"_id\":{\"_data\":\"token2\"},\"operationType\":\"insert\"}";
    let evt: ChangeStreamEvent<Document> = serde_json::from_str(evt_json).unwrap();
    let _ = tx.send(Ok(evt)).await;
    tokio::time::sleep(Duration::from_millis(30)).await;

    assert_eq!(*calls.lock().await, 1);
    assert!(
        map.lock()
            .await
            .get("changestream:resume:shutdown")
            .is_some()
    );
}

#[tokio::test]
async fn test_notify_loop_shutdown() {
    use twilight_model::id::{Id, marker::ChannelMarker};
    let http = std::sync::Arc::new(MockHttp::new());
    let token = CancellationToken::new();
    let handle = notify_loop_mock(
        http.clone(),
        Id::<ChannelMarker>::new(1),
        1,
        "hi",
        || Duration::from_millis(5),
        token.clone(),
    );
    tokio::time::sleep(Duration::from_millis(15)).await;
    token.cancel();
    tokio::time::sleep(Duration::from_millis(20)).await;
    handle.abort();
    let count = http.logs.lock().unwrap().len();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(count > 0);
    assert_eq!(http.logs.lock().unwrap().len(), count);
}
