use std::time::Duration;

use mongodb::{
    Collection,
    change_stream::event::{ChangeStreamEvent, ResumeToken},
    error::ErrorKind,
    options::ChangeStreamOptions,
};
use serde::de::DeserializeOwned;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::dbs::redis::{redis_delete, redis_get, redis_set};

use deadpool_redis::Pool;

fn is_unusable_resume_token(error: &mongodb::error::Error) -> bool {
    matches!(
        error.kind.as_ref(),
        ErrorKind::Command(command) if matches!(command.code, 260 | 280 | 286)
    )
}

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

async fn persist_resume_token(
    pool: &Pool,
    redis_key: &str,
    coll_name: &str,
    resume_token: Option<ResumeToken>,
) {
    let Some(resume_token) = resume_token else {
        return;
    };
    match serde_json::to_string(&resume_token) {
        Ok(token_str) => redis_set(pool, redis_key, &token_str).await,
        Err(e) => {
            tracing::warn!(collection = coll_name, error = %e, "failed to serialize resume token")
        }
    }
}

pub async fn spawn_watcher<T, F, Fut, R, RFut>(
    coll: Collection<T>,
    options: ChangeStreamOptions,
    pool: Pool,
    mut handler: F,
    mut recover_continuity: R,
    token: CancellationToken,
) -> anyhow::Result<()>
where
    T: DeserializeOwned + Unpin + Send + Sync + 'static,
    F: FnMut(ChangeStreamEvent<T>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
    R: FnMut() -> RFut + Send + 'static,
    RFut: Future<Output = anyhow::Result<usize>> + Send + 'static,
{
    let redis_key = format!("changestream:resume:{}", coll.name());
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        while !token.is_cancelled() {
            let mut builder = coll
                .watch()
                .with_options(options.clone());
            let resume_token = load_resume_token(&pool, &redis_key, coll.name()).await;
            let resume_requested = resume_token.is_some();
            if let Some(resume_token) = resume_token {
                builder = builder.resume_after(resume_token);
            }

            let (mut stream, continuity_lost) = match builder.await {
                Ok(stream) => (stream, !resume_requested),
                Err(e) => {
                    if resume_requested && is_unusable_resume_token(&e) {
                        tracing::warn!(collection = coll.name(), error = %e, "resume token invalid, starting from now");
                        redis_delete(&pool, &redis_key).await;
                        match coll
                            .watch()
                            .with_options(options.clone())
                            .await
                        {
                            Ok(stream) => (stream, true),
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

            if continuity_lost {
                match recover_continuity().await {
                    Ok(deleted) => {
                        tracing::warn!(
                            collection = coll.name(),
                            deleted,
                            "change stream continuity unavailable; purged collection caches"
                        );
                    }
                    Err(e) => {
                        tracing::error!(collection = coll.name(), error = %e, "failed to purge caches after change stream continuity loss, retrying");
                        tokio::select! {
                            _ = token.cancelled() => break,
                            _ = sleep(backoff) => {}
                        }
                        backoff = (backoff * 2).min(Duration::from_secs(60));
                        continue;
                    }
                }
            }
            backoff = Duration::from_secs(1);

            while !token.is_cancelled() && stream.is_alive() {
                let evt_res = tokio::select! {
                    _ = token.cancelled() => break,
                    evt = stream.next_if_any() => evt,
                };
                match evt_res {
                    Ok(evt) => {
                        if let Some(evt) = evt {
                            handler(evt).await;
                        }
                        persist_resume_token(
                            &pool,
                            &redis_key,
                            coll.name(),
                            stream.resume_token(),
                        )
                        .await;
                    }
                    Err(e) if is_unusable_resume_token(&e) => {
                        tracing::warn!(collection = coll.name(), error = %e, "change stream continuity lost, restarting from now");
                        redis_delete(&pool, &redis_key).await;
                        break;
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
