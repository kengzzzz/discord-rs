use twilight_model::id::{Id, marker::UserMarker};

use super::ChatEntry;
#[cfg(test)]
use once_cell::sync::Lazy;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use tokio::sync::RwLock;

#[cfg(not(test))]
use crate::configs::CACHE_PREFIX;
use crate::context::Context;
#[cfg(not(test))]
use crate::dbs::redis::{redis_delete, redis_get, redis_set};
use std::sync::Arc;

#[cfg(test)]
static HISTORY_STORE: Lazy<RwLock<HashMap<u64, Vec<ChatEntry>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
#[cfg(test)]
static PROMPT_STORE: Lazy<RwLock<HashMap<u64, String>>> = Lazy::new(|| RwLock::new(HashMap::new()));

#[cfg(not(test))]
async fn history_key(user: Id<UserMarker>) -> String {
    format!("{CACHE_PREFIX}:ai:history:{}", user.get())
}

#[cfg(not(test))]
async fn prompt_key(user: Id<UserMarker>) -> String {
    format!("{CACHE_PREFIX}:ai:prompt:{}", user.get())
}

pub(super) async fn load_history(user: Id<UserMarker>) -> Vec<ChatEntry> {
    #[cfg(test)]
    {
        return HISTORY_STORE
            .read()
            .await
            .get(&user.get())
            .cloned()
            .unwrap_or_default();
    }
    #[cfg(not(test))]
    {
        let key = history_key(user).await;
        redis_get::<Vec<ChatEntry>>(&key).await.unwrap_or_default()
    }
}

pub(super) async fn store_history(user: Id<UserMarker>, hist: &[ChatEntry]) {
    #[cfg(test)]
    {
        HISTORY_STORE
            .write()
            .await
            .insert(user.get(), hist.to_vec());
    }
    #[cfg(not(test))]
    {
        let key = history_key(user).await;
        redis_set(&key, &hist.to_vec()).await;
    }
}

pub(super) async fn get_prompt(ctx: Arc<Context>, user: Id<UserMarker>) -> Option<String> {
    #[cfg(test)]
    {
        let _ = ctx; // silence unused
        return PROMPT_STORE.read().await.get(&user.get()).cloned();
    }
    #[cfg(not(test))]
    {
        let key = prompt_key(user).await;
        if let Some(prompt) = redis_get::<String>(&key).await {
            return Some(prompt);
        }

        use mongodb::bson::doc;

        if let Ok(Some(record)) = ctx
            .mongo
            .ai_prompts
            .find_one(doc! {"user_id": user.get() as i64})
            .await
        {
            redis_set(&key, &record.prompt).await;
            return Some(record.prompt);
        }

        None
    }
}

pub(super) async fn clear_history(user: Id<UserMarker>) {
    #[cfg(test)]
    {
        HISTORY_STORE.write().await.remove(&user.get());
    }
    #[cfg(not(test))]
    {
        let key = history_key(user).await;
        redis_delete(&key).await;
    }
}

pub(super) async fn set_prompt(ctx: Arc<Context>, user: Id<UserMarker>, prompt: String) {
    #[cfg(test)]
    {
        let _ = &ctx;
        PROMPT_STORE.write().await.insert(user.get(), prompt);
    }
    #[cfg(not(test))]
    {
        use crate::dbs::mongo::ai_prompt::AiPrompt;
        use mongodb::bson::{doc, to_bson};

        if let Ok(bson) = to_bson(&AiPrompt {
            id: None,
            user_id: user.get(),
            prompt: prompt.clone(),
        }) {
            let _ = ctx
                .mongo
                .ai_prompts
                .update_one(doc! {"user_id": user.get() as i64}, doc! {"$set": bson})
                .upsert(true)
                .await;
        }
    }
}

pub(super) async fn purge_prompt_cache(user_id: u64) {
    #[cfg(test)]
    {
        PROMPT_STORE.write().await.remove(&user_id);
    }
    #[cfg(not(test))]
    {
        let key = format!("{CACHE_PREFIX}:ai:prompt:{user_id}");
        redis_delete(&key).await;
    }
}

#[cfg(test)]
pub(super) async fn load_history_test(user: Id<UserMarker>) -> Vec<ChatEntry> {
    HISTORY_STORE
        .read()
        .await
        .get(&user.get())
        .cloned()
        .unwrap_or(Vec::with_capacity(21))
}

#[cfg(test)]
pub(super) async fn set_history_test(user: Id<UserMarker>, hist: Vec<ChatEntry>) {
    HISTORY_STORE.write().await.insert(user.get(), hist);
}

#[cfg(test)]
pub(super) async fn get_prompt_test(user: Id<UserMarker>) -> Option<String> {
    PROMPT_STORE.read().await.get(&user.get()).cloned()
}
