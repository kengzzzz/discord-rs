use std::time::Duration;

use futures::StreamExt;
use mongodb::{
    Collection,
    change_stream::event::{ChangeStreamEvent, ResumeToken},
    options::ChangeStreamOptions,
};
use serde::de::DeserializeOwned;
use tokio::time::sleep;

use crate::dbs::redis::{redis_get, redis_set};

pub async fn spawn_watcher<T, F, Fut>(
    coll: Collection<T>,
    options: ChangeStreamOptions,
    mut handler: F,
) -> anyhow::Result<()>
where
    T: DeserializeOwned + Unpin + Send + Sync + 'static,
    F: FnMut(ChangeStreamEvent<T>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let redis_key = format!("changestream:resume:{}", coll.name());
    tokio::spawn(async move {
        loop {
            let mut builder = coll.watch().with_options(options.clone());
            if let Some(token_str) = redis_get::<String>(&redis_key).await {
                match serde_json::from_str::<ResumeToken>(&token_str) {
                    Ok(token) => builder = builder.resume_after(token),
                    Err(e) => {
                        tracing::warn!(collection = coll.name(), error = %e, "invalid resume token")
                    }
                }
            }

            let mut stream = match builder.await {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::error!(collection = coll.name(), error = %e, "failed to start change stream");
                    sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

            while let Some(evt_res) = stream.next().await {
                match evt_res {
                    Ok(evt) => {
                        let resume_token = evt.id.clone();
                        handler(evt).await;
                        if let Ok(token_str) = serde_json::to_string(&resume_token) {
                            redis_set(&redis_key, &token_str).await;
                        } else {
                            tracing::warn!(
                                collection = coll.name(),
                                "failed to serialize resume token"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(collection = coll.name(), error = %e, "change stream error, restarting");
                        break;
                    }
                }
            }

            sleep(Duration::from_secs(5)).await;
        }
    });
    Ok(())
}
