use deadpool_redis::Pool;
use twilight_model::{channel::Attachment, id::Id, id::marker::UserMarker};

use self::history as hist;
use self::models::ChatEntry;
mod interaction;
use crate::context::Context;
use crate::services::ai::rate_limit::check_rate_limit;
use std::collections::VecDeque;
use std::sync::Arc;

pub mod attachments;
pub mod client;
pub mod embed;
pub(crate) mod history;
pub mod models;
mod rate_limit;

const MAX_HISTORY: usize = 20;
const KEEP_RECENT: usize = 2;

pub struct AiInteraction<'a> {
    pub user_id: Id<UserMarker>,
    pub user_name: &'a str,
    pub message: &'a str,
    pub attachments: Vec<Attachment>,
    pub ref_text: Option<&'a str>,
    pub ref_attachments: Vec<Attachment>,
    pub ref_author: Option<&'a str>,
}

pub struct AiService;

impl AiService {
    pub async fn clear_history(pool: &Pool, user: Id<UserMarker>) {
        hist::clear_history(pool, user).await;
    }

    pub async fn set_prompt(ctx: &Arc<Context>, user: Id<UserMarker>, prompt: String) {
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

    async fn get_prompt(ctx: &Arc<Context>, user: Id<UserMarker>) -> Option<String> {
        hist::get_prompt(ctx, user).await
    }

    pub async fn handle_interaction<C>(
        ctx: &Arc<Context>,
        client: &Arc<C>,
        interaction: AiInteraction<'_>,
    ) -> anyhow::Result<String>
    where
        C: client::AiClient + Send + Sync + 'static,
    {
        let AiInteraction {
            user_id,
            user_name,
            message,
            attachments,
            ref_text,
            ref_attachments,
            ref_author,
        } = interaction;

        let mut history = Self::load_history(&ctx.redis, user_id).await;

        interaction::spawn_summary(Arc::clone(client), ctx, user_id, user_name, &history).await;

        let prompt = Self::get_prompt(ctx, user_id).await;

        let args = interaction::BuildRequest {
            ctx,
            prompt,
            user_name,
            message,
            history: &history,
            attachments,
            ref_text,
            ref_attachments,
            ref_author,
        };
        let (system, contents, attachment_urls, ref_attachment_urls) =
            interaction::build_request(args).await?;

        let text = interaction::process_response(client.as_ref(), &system, contents).await?;

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

    pub async fn check_rate_limit(ctx: &Arc<Context>, user: Id<UserMarker>) -> Option<u64> {
        check_rate_limit(ctx, user).await
    }
}
