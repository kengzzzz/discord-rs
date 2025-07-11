use anyhow::Context;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use google_ai_rs::{Content, Part};
#[cfg(test)]
use once_cell::sync::OnceCell as SyncOnceCell;
use reqwest::{Body, header::CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use twilight_model::{channel::Attachment, id::Id, id::marker::UserMarker};

use self::client::{MODELS, extract_text};
use self::history as hist;
use crate::{configs::google::GOOGLE_CONFIGS, services::http::HttpService};

mod client;
mod history;

#[cfg(test)]
static GENERATE_OVERRIDE: SyncOnceCell<
    Box<dyn Fn(Vec<Content>) -> google_ai_rs::genai::Response + Send + Sync>,
> = SyncOnceCell::new();
#[cfg(test)]
#[allow(clippy::type_complexity)]
static SUMMARIZE_OVERRIDE: SyncOnceCell<Box<dyn Fn(&[ChatEntry]) -> String + Send + Sync>> =
    SyncOnceCell::new();

const MAX_HISTORY: usize = 20;
const KEEP_RECENT: usize = 6;

const INLINE_LIMIT: u64 = 20 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct ChatEntry {
    role: String,
    text: String,
    #[serde(default)]
    attachments: Vec<String>,
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn new_entry(role: &str, text: &str) -> ChatEntry {
    ChatEntry {
        role: role.to_string(),
        text: text.to_string(),
        attachments: Vec::new(),
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn entry_role(e: &ChatEntry) -> &str {
    &e.role
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn entry_text(e: &ChatEntry) -> &str {
    &e.text
}

pub struct AiService;

impl AiService {
    pub async fn clear_history(user: Id<UserMarker>) {
        hist::clear_history(user).await;
    }

    pub async fn set_prompt(user: Id<UserMarker>, prompt: String) {
        hist::set_prompt(user, prompt).await;
    }

    pub async fn purge_prompt_cache(user_id: u64) {
        hist::purge_prompt_cache(user_id).await;
    }

    async fn load_history(user: Id<UserMarker>) -> Vec<ChatEntry> {
        hist::load_history(user).await
    }

    async fn store_history(user: Id<UserMarker>, histv: &[ChatEntry]) {
        hist::store_history(user, histv).await;
    }

    async fn get_prompt(user: Id<UserMarker>) -> Option<String> {
        hist::get_prompt(user).await
    }

    pub async fn handle_interaction(
        user_id: Id<UserMarker>,
        user_name: &str,
        message: &str,
        attachments: Vec<Attachment>,
    ) -> anyhow::Result<String> {
        let mut history = Self::load_history(user_id).await;

        if history.len() > MAX_HISTORY {
            if let Ok(summary) = client::summarize(&history).await {
                let start = history.len().saturating_sub(KEEP_RECENT);
                let mut new_history = Vec::with_capacity(MAX_HISTORY + 1);
                new_history.push(ChatEntry {
                    role: "system".to_string(),
                    text: format!("Summary so far: {summary}"),
                    attachments: Vec::new(),
                });
                new_history.extend(history[start..].to_vec());
                history = new_history;
            }
        }

        let prompt = Self::get_prompt(user_id).await;

        let mut system = format!(
            "{}\nYou are chatting with {user_name}",
            GOOGLE_CONFIGS.base_prompt
        );
        if let Some(p) = prompt {
            system.push_str("\n\nUser instructions:\n");
            system.push_str(&p);
        }

        let mut contents: Vec<Content> = history
            .iter()
            .map(|c| {
                let mut text = c.text.clone();
                for url in &c.attachments {
                    text.push_str("\nAttachment: ");
                    text.push_str(url);
                }
                Content::from((text.as_str(),))
            })
            .collect();

        let mut parts = vec![Part::text(message)];
        let mut attachment_urls = Vec::new();
        for a in attachments {
            if let (Some(ct), Ok(resp)) =
                (a.content_type.as_deref(), HttpService::get(&a.url).await)
            {
                if a.size > INLINE_LIMIT {
                    let stream = Body::wrap_stream(resp.bytes_stream());
                    let upload_url = reqwest::Url::parse_with_params(
                        "https://generativelanguage.googleapis.com/upload/v1beta/files",
                        &[("uploadType", "media")],
                    )?;
                    let mut headers = HeaderMap::new();
                    headers.append(
                        HeaderName::from_str("X-Goog-Api-Key")?,
                        HeaderValue::from_str(GOOGLE_CONFIGS.api_key.as_str())?,
                    );
                    if let Some(content_type) = &a.content_type {
                        headers.append(CONTENT_TYPE, HeaderValue::from_str(content_type.as_str())?);
                    }
                    let resp = HttpService::post(upload_url)
                        .headers(headers)
                        .body(stream)
                        .send()
                        .await?
                        .error_for_status()?;
                    let json: serde_json::Value = resp.json().await?;
                    let uri = json["file"]["uri"].as_str().context("Missing file uri")?;
                    parts.push(Part::file_data(ct, uri));
                    attachment_urls.push(uri.to_string());
                } else if let Ok(bytes) = resp.bytes().await {
                    parts.push(Part::blob(ct, bytes.to_vec()));
                    attachment_urls.push(a.url.clone());
                }
            }
        }

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
                    .with_system_instruction(system.clone());
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

        history.push(ChatEntry {
            role: "user".into(),
            text: message.to_owned(),
            attachments: attachment_urls,
        });
        history.push(ChatEntry {
            role: "model".into(),
            text: text.clone(),
            attachments: Vec::new(),
        });

        Self::store_history(user_id, &history).await;

        Ok(text)
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn set_generate_override<F>(f: F)
where
    F: Fn(Vec<Content>) -> google_ai_rs::genai::Response + Send + Sync + 'static,
{
    let _ = GENERATE_OVERRIDE.set(Box::new(f));
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn set_summarize_override<F>(f: F)
where
    F: Fn(&[ChatEntry]) -> String + Send + Sync + 'static,
{
    let _ = SUMMARIZE_OVERRIDE.set(Box::new(f));
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn load_history_test(user: Id<UserMarker>) -> Vec<ChatEntry> {
    hist::load_history_test(user).await
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn set_history_test(user: Id<UserMarker>, histv: Vec<ChatEntry>) {
    hist::set_history_test(user, histv).await;
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn get_prompt_test(user: Id<UserMarker>) -> Option<String> {
    hist::get_prompt_test(user).await
}
