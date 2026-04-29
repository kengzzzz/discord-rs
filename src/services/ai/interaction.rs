use super::{
    KEEP_RECENT, MAX_HISTORY, attachments,
    client::{self, MODELS, extract_text},
    models::ChatEntry,
    scheduler::{AdmissionConfig, AiOperation, AiScheduler},
};
use crate::{configs::google::GOOGLE_CONFIGS, services::ai::history::parse_history};
use crate::{context::Context, services::ai::history};
use once_cell::sync::Lazy;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use twilight_model::{
    channel::Attachment,
    id::{Id, marker::UserMarker},
};

use crate::services::ai::genai::{Content, Part};

pub(super) struct BuildRequest<'a> {
    pub ctx: &'a Arc<Context>,
    pub prompt: Option<String>,
    pub user_name: &'a str,
    pub message: &'a str,
    pub history: &'a VecDeque<ChatEntry>,
    pub attachments: Vec<Attachment>,
    pub ref_text: Option<&'a str>,
    pub ref_attachments: Vec<Attachment>,
    pub ref_author: Option<&'a str>,
}

pub(super) static RUNNING: Lazy<RwLock<HashSet<u64>>> = Lazy::new(|| RwLock::new(HashSet::new()));

pub(super) async fn spawn_summary<C>(
    client: Arc<C>,
    scheduler: AiScheduler,
    ctx: &Arc<Context>,
    user_id: Id<UserMarker>,
    user_name: &str,
    history: &VecDeque<ChatEntry>,
) where
    C: client::AiClient + Send + Sync + 'static,
{
    if history.len() <= MAX_HISTORY {
        return;
    }

    let uid = user_id.get();
    {
        let mut guard = RUNNING.write().await;
        if !guard.insert(uid) {
            return;
        }
    }

    let ctx = ctx.clone();
    let user_name = user_name.to_string();
    let mut history = history.clone();
    let client_clone = client.clone();
    tokio::spawn(async move {
        if let Ok(summary) = client::summarize(
            client_clone.as_ref(),
            &scheduler,
            &mut history,
            &user_name,
        )
        .await
        {
            let mut latest = history::load_history(&ctx.redis, user_id).await;
            let remove = history
                .len()
                .saturating_sub(KEEP_RECENT);
            for _ in 0..remove {
                if latest.is_empty() {
                    break;
                }
                latest.pop_front();
            }
            latest.push_front(ChatEntry::new(
                "model".to_string(),
                format!("Summary so far:\n{summary}"),
                Vec::new(),
                None,
                None,
                None,
            ));
            history::store_history(&ctx.redis, user_id, &latest).await;
        }

        RUNNING.write().await.remove(&uid);
    });
}

pub(super) async fn build_request(
    args: BuildRequest<'_>,
) -> anyhow::Result<(String, Vec<Content>, Vec<String>, Vec<String>)> {
    let BuildRequest {
        ctx,
        prompt,
        user_name,
        message,
        history,
        attachments,
        ref_text: _ref_text,
        ref_attachments,
        ref_author,
    } = args;

    let mut system = format!(
        "{}\nYou are chatting with {user_name}",
        GOOGLE_CONFIGS.base_prompt
    );
    if let Some(p) = prompt {
        system.push_str("\n\nUser instructions:\n");
        system.push_str(&p);
    }

    let mut contents = parse_history(history, user_name).await;

    let mut parts = vec![Part::text(message)];
    let attachment_urls =
        attachments::append_attachments(&ctx.reqwest, &mut parts, attachments, user_name).await?;
    let ref_owner = ref_author.unwrap_or("referenced user");
    let ref_attachment_urls = attachments::append_attachments(
        &ctx.reqwest,
        &mut parts,
        ref_attachments,
        ref_owner,
    )
    .await?;

    contents.push(Content::from(parts));

    Ok((
        system,
        contents,
        attachment_urls,
        ref_attachment_urls,
    ))
}

pub(super) async fn process_response<C>(
    client: &C,
    scheduler: &AiScheduler,
    system: &str,
    contents: Vec<Content>,
) -> anyhow::Result<String>
where
    C: client::AiClient + Send + Sync,
{
    for spec in MODELS {
        let guard = match scheduler
            .acquire(
                spec.name,
                AiOperation::Chat,
                AdmissionConfig { rpm_limit: spec.rpm_limit, queue_timeout: spec.queue_timeout },
            )
            .await
        {
            Ok(guard) => guard,
            Err(e) => {
                tracing::warn!(model = spec.name, error = %e, "model queue failed");
                continue;
            }
        };

        match client::generate_with_retries(client, spec.name, system, contents.clone()).await {
            Ok(r) => return Ok(extract_text(r)),
            Err(e) => {
                if client::is_retryable(&e) {
                    guard.cool_down(spec.cooldown).await;
                }
                tracing::warn!(model = spec.name, error = %e, "model failed");
            }
        };
    }

    Err(anyhow::anyhow!("all models failed"))
}
