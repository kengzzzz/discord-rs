use super::{
    KEEP_RECENT, MAX_HISTORY, attachments,
    client::{self, MODELS, extract_text},
    models::ChatEntry,
};
use crate::configs::google::GOOGLE_CONFIGS;
use crate::context::Context;
use google_ai_rs::{Content, Part};
use std::collections::VecDeque;
use std::sync::Arc;
use twilight_model::channel::Attachment;

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

pub(super) async fn summarize_history(history: &mut VecDeque<ChatEntry>) {
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

    let now = chrono::Utc::now();
    let mut contents: Vec<Content> = history
        .iter()
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
        .collect();

    let mut parts = vec![Part::text(message)];
    let attachment_urls =
        attachments::append_attachments(ctx, &mut parts, attachments, user_name).await?;
    let ref_owner = ref_author.unwrap_or("referenced user");
    let ref_attachment_urls =
        attachments::append_attachments(ctx, &mut parts, ref_attachments, ref_owner).await?;

    contents.push(Content::from(parts));

    Ok((system, contents, attachment_urls, ref_attachment_urls))
}

pub(super) async fn process_response(
    system: &str,
    contents: Vec<Content>,
) -> anyhow::Result<String> {
    let mut response = {
        #[cfg(test)]
        {
            super::tests::GENERATE_OVERRIDE
                .get()
                .map(|f| f(contents.clone()))
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
                .with_system_instruction(system);
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
    Ok(extract_text(response))
}
