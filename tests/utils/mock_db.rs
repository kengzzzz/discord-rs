#![allow(dead_code)]

use futures::Stream;
use futures::StreamExt;
use mongodb::change_stream::event::ChangeStreamEvent;
use serde::de::DeserializeOwned;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

pub fn init_mock() -> Arc<Mutex<HashMap<String, String>>> {
    Arc::new(Mutex::new(HashMap::new()))
}

async fn redis_set_mock<T: serde::Serialize + Sync>(
    map: &Arc<Mutex<HashMap<String, String>>>,
    key: &str,
    value: &T,
) {
    if let Ok(json) = serde_json::to_string(value) {
        map.lock().await.insert(key.to_string(), json);
    }
}

pub async fn spawn_watcher_mock<T, St, F, Fut>(
    map: &Arc<Mutex<HashMap<String, String>>>,
    name: &str,
    mut stream: St,
    mut handler: F,
    token: CancellationToken,
) -> anyhow::Result<()>
where
    T: DeserializeOwned + Unpin + Send + Sync + 'static,
    St: Stream<Item = mongodb::error::Result<ChangeStreamEvent<T>>> + Unpin + Send + 'static,
    F: FnMut(ChangeStreamEvent<T>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let redis_key = format!("changestream:resume:{name}");
    let map = map.clone();
    tokio::spawn(async move {
        while !token.is_cancelled() {
            while let Some(evt_res) = tokio::select! {
                _ = token.cancelled() => None,
                evt = stream.next() => evt,
            } {
                match evt_res {
                    Ok(evt) => {
                        let resume_token = evt.id.clone();
                        handler(evt).await;
                        redis_set_mock(&map, &redis_key, &resume_token).await;
                    }
                    Err(_) => break,
                }
            }
            tokio::select! { _ = token.cancelled() => break, _ = sleep(Duration::from_millis(10)) => {} }
        }
    });
    Ok(())
}
