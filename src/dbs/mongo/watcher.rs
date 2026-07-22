use std::time::Duration;

use futures::StreamExt;
use mongodb::{
    Collection,
    change_stream::event::{ChangeStreamEvent, ResumeToken},
    options::ChangeStreamOptions,
};
use serde::de::DeserializeOwned;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::{
    dbs::redis::{redis_delete, redis_get, redis_set},
    utils::ascii::ascii_contains_icase,
};

use deadpool_redis::Pool;

async fn load_resume_token(pool: &Pool, redis_key: &str, coll_name: &str) -> Option<ResumeToken> {
    let token_str = redis_get::<String>(pool, redis_key).await?;
    match serde_json::from_str::<ResumeToken>(&token_str) {
        Ok(token) => Some(token),
        Err(e) => {
            tracing::warn!(collection = coll_name, error = %e, "invalid resume token, starting from now");
            redis_delete(pool, redis_key).await;
            None
        }
    }
}

pub async fn spawn_watcher<T, F, Fut>(
    coll: Collection<T>,
    options: ChangeStreamOptions,
    pool: Pool,
    mut handler: F,
    token: CancellationToken,
) -> anyhow::Result<()>
where
    T: DeserializeOwned + Unpin + Send + Sync + 'static,
    F: FnMut(ChangeStreamEvent<T>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let redis_key = format!("changestream:resume:{}", coll.name());
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        while !token.is_cancelled() {
            let mut builder = coll
                .watch()
                .with_options(options.clone());
            if let Some(resume_token) = load_resume_token(&pool, &redis_key, coll.name()).await {
                builder = builder.resume_after(resume_token);
            }

            let mut stream = match builder.await {
                Ok(stream) => {
                    backoff = Duration::from_secs(1);
                    stream
                }
                Err(e) => {
                    if ascii_contains_icase(&e.to_string(), "resume") {
                        tracing::warn!(collection = coll.name(), error = %e, "resume token invalid, starting from now");
                        redis_delete(&pool, &redis_key).await;
                        match coll
                            .watch()
                            .with_options(options.clone())
                            .await
                        {
                            Ok(stream) => {
                                backoff = Duration::from_secs(1);
                                stream
                            }
                            Err(e) => {
                                tracing::error!(collection = coll.name(), error = %e, "failed to start change stream, retrying");
                                tokio::select! {
                                    _ = token.cancelled() => break,
                                    _ = sleep(backoff) => {}
                                }
                                backoff = (backoff * 2).min(Duration::from_secs(60));
                                continue;
                            }
                        }
                    } else {
                        tracing::error!(collection = coll.name(), error = %e, "failed to start change stream, retrying");
                        tokio::select! {
                            _ = token.cancelled() => break,
                            _ = sleep(backoff) => {}
                        }
                        backoff = (backoff * 2).min(Duration::from_secs(60));
                        continue;
                    }
                }
            };

            while !token.is_cancelled() {
                let evt_res = tokio::select! {
                    _ = token.cancelled() => None,
                    evt = stream.next() => evt,
                };
                let Some(evt_res) = evt_res else { break };
                match evt_res {
                    Ok(evt) => {
                        let resume_token = evt.id.clone();
                        handler(evt).await;
                        if let Ok(token_str) = serde_json::to_string(&resume_token) {
                            redis_set(&pool, &redis_key, &token_str).await;
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

            tracing::info!(collection = coll.name(), delay = ?backoff, "restart change stream");
            tokio::select! {
                _ = token.cancelled() => break,
                _ = sleep(backoff) => {}
            }
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    });
    Ok(())
}

#[cfg(test)]
#[path = "tests/watcher.rs"]
mod tests;
