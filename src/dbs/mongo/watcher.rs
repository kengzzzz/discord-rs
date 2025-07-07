use futures::StreamExt;
use mongodb::{
    Collection,
    change_stream::event::{ChangeStreamEvent, ResumeToken},
    options::ChangeStreamOptions,
};
use serde::de::DeserializeOwned;

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
    let mut builder = coll.watch().with_options(options);
    if let Some(token_str) = redis_get::<String>(&redis_key).await {
        let token: ResumeToken = serde_json::from_str(&token_str)?;
        builder = builder.resume_after(token);
    }
    let mut stream = builder.await?;
    tokio::spawn(async move {
        while let Some(Ok(evt)) = stream.next().await {
            let resume_token = evt.id.clone();
            handler(evt).await;
            let token_str =
                serde_json::to_string(&resume_token).expect("Failed to serialize resume token");
            redis_set(&redis_key, &token_str).await;
        }
    });
    Ok(())
}
