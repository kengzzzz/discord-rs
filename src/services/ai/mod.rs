#[cfg(test)]
use self::tests::GENERATE_OVERRIDE;
use deadpool_redis::Pool;
use google_ai_rs::{Content, Part};
use twilight_model::{channel::Attachment, id::Id, id::marker::UserMarker};

use self::client::{MODELS, extract_text};
use self::history as hist;
use self::models::ChatEntry;
use crate::{configs::google::GOOGLE_CONFIGS, context::Context};
use std::collections::VecDeque;
use std::sync::Arc;

pub mod attachments;
mod client;
pub mod embed;
pub(crate) mod history;
pub mod models;

const MAX_HISTORY: usize = 20;
const KEEP_RECENT: usize = 6;

pub struct AiService;

impl AiService {
    pub async fn clear_history(pool: &Pool, user: Id<UserMarker>) {
        hist::clear_history(pool, user).await;
    }

    pub async fn set_prompt(ctx: Arc<Context>, user: Id<UserMarker>, prompt: String) {
        hist::set_prompt(ctx, user, prompt).await;
    }

    pub async fn purge_prompt_cache(pool: &Pool, user_id: u64) {
        hist::purge_prompt_cache(pool, user_id).await;
    }

    async fn load_history(pool: &Pool, user: Id<UserMarker>) -> VecDeque<ChatEntry> {
        hist::load_history(pool, user).await
    }

    async fn store_history(pool: &Pool, user: Id<UserMarker>, histv: &VecDeque<ChatEntry>) {
        hist::store_history(pool, user, histv).await;
    }

    async fn get_prompt(ctx: Arc<Context>, user: Id<UserMarker>) -> Option<String> {
        hist::get_prompt(ctx, user).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn handle_interaction(
        ctx: Arc<Context>,
        user_id: Id<UserMarker>,
        user_name: &str,
        message: &str,
        attachments: Vec<Attachment>,
        ref_text: Option<&str>,
        ref_attachments: Vec<Attachment>,
        ref_author: Option<&str>,
    ) -> anyhow::Result<String> {
        let mut history = Self::load_history(&ctx.redis, user_id).await;

        if history.len() > MAX_HISTORY {
            if let Ok(summary) = client::summarize(history.make_contiguous()).await {
                while history.len() > KEEP_RECENT {
                    history.pop_front();
                }
                history.push_front(ChatEntry::new(
                    "user".to_string(),
                    format!("Summary so far: {summary}"),
                    Vec::new(),
                    None,
                    None,
                    None,
                ));
            }
        }

        let prompt = Self::get_prompt(ctx.clone(), user_id).await;

        let mut system = format!(
            "{}\nYou are chatting with {user_name}",
            GOOGLE_CONFIGS.base_prompt
        );
        if let Some(p) = prompt {
            system.push_str("\n\nUser instructions:\n");
            system.push_str(&p);
        }

        let now = chrono::Utc::now();
        let mut contents: Vec<Content> = history
            .iter()
            .map(|c| {
                let mut parts = vec![Part::text(&c.text)];
                let expired = now - c.created_at > chrono::Duration::hours(48);
                for url in &c.attachments {
                    if expired {
                        let label = format!(
                            "Attachment from {user_name} is expired and no longer accessible."
                        );
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
                            let label = format!(
                                "Attachment from {owner} is expired and no longer accessible."
                            );
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
            .collect();

        let mut parts = vec![Part::text(message)];
        let attachment_urls =
            attachments::append_attachments(&ctx, &mut parts, attachments, user_name).await?;
        let ref_owner = ref_author.unwrap_or("referenced user");
        let ref_attachment_urls =
            attachments::append_attachments(&ctx, &mut parts, ref_attachments, ref_owner).await?;

        contents.push(Content::from(parts));

        let mut response = {
            #[cfg(test)]
            {
                GENERATE_OVERRIDE.get().map(|f| f(contents.clone()))
            }
            #[cfg(not(test))]
            {
                None
            }
        };

        if response.is_none() {
            let client = client::client().await?;
            for name in MODELS {
                let m = client
                    .generative_model(name)
                    .with_system_instruction(system.as_str());
                match m.generate_content(contents.clone()).await {
                    Ok(r) => {
                        response = Some(r);
                        break;
                    }
                    Err(e) => tracing::warn!(model = %name, error = %e, "model failed"),
                }
            }
        }
        let response = response.ok_or_else(|| anyhow::anyhow!("all models failed"))?;

        let text = extract_text(response);

        history.push_back(ChatEntry::new(
            "user".into(),
            message.to_owned(),
            attachment_urls,
            ref_text.map(|t| t.to_string()),
            if ref_attachment_urls.is_empty() {
                None
            } else {
                Some(ref_attachment_urls)
            },
            ref_author.map(|t| t.to_string()),
        ));
        history.push_back(ChatEntry::new(
            "model".into(),
            text.clone(),
            Vec::new(),
            None,
            None,
            None,
        ));
        Self::store_history(&ctx.redis, user_id, &history).await;

        Ok(text)
    }
}

#[cfg(test)]
pub(crate) mod tests;
