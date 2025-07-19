use chrono::Utc;
use deadpool_redis::Pool;
use google_ai_rs::{Content, Part};
use mongodb::bson::{doc, to_bson};
use twilight_model::id::{Id, marker::UserMarker};

use super::models::ChatEntry;
use crate::{
    configs::CACHE_PREFIX,
    context::Context,
    dbs::mongo::models::ai_prompt::AiPrompt,
    dbs::redis::{redis_delete, redis_get, redis_set},
};
use std::{collections::VecDeque, sync::Arc};

async fn history_key(user: Id<UserMarker>) -> String {
    format!("{CACHE_PREFIX}:ai:history:{}", user.get())
}

async fn prompt_key(user: Id<UserMarker>) -> String {
    format!("{CACHE_PREFIX}:ai:prompt:{}", user.get())
}

pub(crate) async fn load_history(_pool: &Pool, user: Id<UserMarker>) -> VecDeque<ChatEntry> {
    let key = history_key(user).await;
    redis_get::<VecDeque<ChatEntry>>(_pool, &key)
        .await
        .unwrap_or_default()
}

pub(crate) async fn store_history(_pool: &Pool, user: Id<UserMarker>, hist: &VecDeque<ChatEntry>) {
    let key = history_key(user).await;
    redis_set(_pool, &key, hist).await;
}

pub(crate) async fn get_prompt(ctx: &Arc<Context>, user: Id<UserMarker>) -> Option<String> {
    let key = prompt_key(user).await;
    if let Some(prompt) = redis_get::<String>(&ctx.redis, &key).await {
        return Some(prompt);
    }
    if let Ok(Some(record)) = ctx
        .mongo
        .ai_prompts
        .find_one(doc! {"user_id": user.get() as i64})
        .await
    {
        redis_set(&ctx.redis, &key, &record.prompt).await;
        return Some(record.prompt);
    }
    None
}

pub(crate) async fn clear_history(_pool: &Pool, user: Id<UserMarker>) {
    let key = history_key(user).await;
    redis_delete(_pool, &key).await;
}

pub(crate) async fn set_prompt(ctx: &Arc<Context>, user: Id<UserMarker>, prompt: String) {
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

pub(crate) async fn purge_prompt_cache(_pool: &Pool, user_id: u64) {
    let key = format!("{CACHE_PREFIX}:ai:prompt:{user_id}");
    redis_delete(_pool, &key).await;
}

pub(crate) async fn parse_history<'a>(
    history: impl IntoIterator<Item = &'a ChatEntry>,
    user_name: &str,
) -> Vec<Content> {
    let now = Utc::now();
    history
        .into_iter()
        .map(|c| {
            let mut parts = vec![Part::text(&c.text)];
            let expired = now - c.created_at > chrono::Duration::hours(48);
            for url in &c.attachments {
                if expired {
                    let label =
                        format!("Attachment from {user_name} is expired and no longer accessible.");
                    parts.push(Part::text(&label));
                } else {
                    let label = format!("Attachment from {user_name}:");
                    parts.push(Part::text(&label));
                    parts.push(Part::file_data("", url));
                }
            }
            if let Some(ref_text) = &c.ref_text {
                let owner = c.ref_author.as_deref().unwrap_or("another user");
                let label = format!("In reply to {owner}:");
                parts.push(Part::text(&label));
                parts.push(Part::text(ref_text));
            }
            if let Some(ref_urls) = &c.ref_attachments {
                let owner = c.ref_author.as_deref().unwrap_or("another user");
                for url in ref_urls {
                    if expired {
                        let label =
                            format!("Attachment from {owner} is expired and no longer accessible.");
                        parts.push(Part::text(&label));
                    } else {
                        let label = format!("Attachment from {owner}:");
                        parts.push(Part::text(&label));
                        parts.push(Part::file_data("", url));
                    }
                }
            }
            Content {
                role: c.role.clone(),
                parts,
            }
        })
        .collect()
}
