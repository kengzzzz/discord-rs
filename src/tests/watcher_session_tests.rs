use mongodb::bson::Document;
use mongodb::change_stream::event::ChangeStreamEvent;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use crate::{
    services::market::{MarketKind, MarketSession, OrderInfo},
    tests::mock_db::{init_mock, spawn_watcher_mock},
};
use std::collections::BTreeMap;

#[tokio::test]
async fn test_watcher_mock() {
    let map = init_mock();
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let token = CancellationToken::new();
    let calls = std::sync::Arc::new(tokio::sync::Mutex::new(0u32));
    let calls_clone = calls.clone();
    spawn_watcher_mock(
        "test",
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
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    token.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(*calls.lock().await, 1);
    let stored = map.lock().await.get("changestream:resume:test").cloned();
    assert!(stored.is_some());
}

#[test]
fn test_market_session_pages() {
    let mut orders = BTreeMap::new();
    let list: Vec<_> = (0..12)
        .map(|i| OrderInfo {
            quantity: 1,
            platinum: i,
            ign: format!("U{i}"),
        })
        .collect();
    orders.insert(0, list);
    let session = MarketSession {
        item: "Item".into(),
        url: "url".into(),
        kind: MarketKind::Buy,
        orders,
        rank: 0,
        page: 1,
        max_rank: None,
    };
    assert_eq!(session.lpage(), 3);
    assert_eq!(session.slice().len(), 5);
    let mut s = session.clone();
    s.page = 3;
    assert_eq!(s.slice().len(), 2);
    s.page = 4;
    assert_eq!(s.slice().len(), 0);
}
