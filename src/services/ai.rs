use google_ai_rs::{Client, Content, Part};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use twilight_model::{channel::Attachment, id::Id, id::marker::UserMarker};

use crate::{
    configs::{CACHE_PREFIX, google::GOOGLE_CONFIGS},
    dbs::redis::{redis_delete, redis_get, redis_set},
};

static CLIENT: OnceCell<Client> = OnceCell::const_new();

const MAX_HISTORY: usize = 20;
const KEEP_RECENT: usize = 6;

const MODELS: &[&str] = &[
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite-preview-06-17",
    "gemini-2.5-flash-preview-tts",
    "gemini-2.0-flash",
    "gemini-2.0-flash-preview-image-generation",
    "gemini-2.0-flash-lite",
];

const SUMMARY_MODELS: &[&str] = &[
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite-preview-06-17",
    "gemini-2.5-flash-preview-tts",
    "gemini-2.0-flash",
    "gemini-2.0-flash-lite",
];

#[derive(Serialize, Deserialize, Clone)]
struct ChatEntry {
    role: String,
    text: String,
    #[serde(default)]
    attachments: Vec<String>,
}

fn extract_text(response: google_ai_rs::genai::Response) -> String {
    response
        .candidates
        .first()
        .and_then(|c| c.content.as_ref())
        .and_then(|c| c.parts.first())
        .and_then(|p| match &p.data {
            Some(google_ai_rs::proto::part::Data::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

async fn summarize(history: &[ChatEntry]) -> anyhow::Result<String> {
    let client = AiService::client().await?;
    let contents: Vec<Content> = history
        .iter()
        .map(|c| Content::from((c.text.as_str(),)))
        .collect();
    let system = "Summarize the conversation so far in a concise form.".to_string();

    for name in SUMMARY_MODELS {
        let model = client
            .generative_model(name)
            .with_system_instruction(system.clone());
        match model.generate_content(contents.clone()).await {
            Ok(resp) => return Ok(extract_text(resp)),
            Err(e) => tracing::warn!(model = %name, error = %e, "summary model failed"),
        }
    }

    Err(anyhow::anyhow!("all models failed to summarize"))
}

pub struct AiService;

impl AiService {
    async fn client() -> anyhow::Result<&'static Client> {
        CLIENT
            .get_or_try_init(|| async {
                Client::new(google_ai_rs::Auth::ApiKey(GOOGLE_CONFIGS.api_key.clone()))
                    .await
                    .map_err(anyhow::Error::msg)
            })
            .await
    }

    async fn history_key(user: Id<UserMarker>) -> String {
        format!("{CACHE_PREFIX}:ai:history:{}", user.get())
    }

    async fn prompt_key(user: Id<UserMarker>) -> String {
        format!("{CACHE_PREFIX}:ai:prompt:{}", user.get())
    }

    async fn load_history(user: Id<UserMarker>) -> Vec<ChatEntry> {
        let key = Self::history_key(user).await;
        redis_get::<Vec<ChatEntry>>(&key).await.unwrap_or_default()
    }

    async fn store_history(user: Id<UserMarker>, hist: &Vec<ChatEntry>) {
        let key = Self::history_key(user).await;
        redis_set(&key, hist).await;
    }

    async fn get_prompt(user: Id<UserMarker>) -> Option<String> {
        let key = Self::prompt_key(user).await;
        redis_get::<String>(&key).await
    }

    pub async fn clear_history(user: Id<UserMarker>) {
        let key = Self::history_key(user).await;
        redis_delete(&key).await;
    }

    pub async fn set_prompt(user: Id<UserMarker>, prompt: String) {
        let key = Self::prompt_key(user).await;
        redis_set(&key, &prompt).await;
    }

    pub async fn handle_interaction(
        user_id: Id<UserMarker>,
        user_name: &str,
        message: &str,
        attachment: Option<Attachment>,
    ) -> anyhow::Result<String> {
        let client = Self::client().await?;

        let mut history = Self::load_history(user_id).await;

        if history.len() > MAX_HISTORY {
            if let Ok(summary) = summarize(&history).await {
                let start = history.len().saturating_sub(KEEP_RECENT);
                let mut new_history = Vec::with_capacity(KEEP_RECENT + 1);
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
        if let Some(a) = attachment {
            if let Some(ct) = a.content_type.as_deref() {
                if a.size > 20 * 1024 * 1024 {
                    parts.push(Part::file_data(ct, &a.url));
                    attachment_urls.push(a.url.clone());
                } else if let Ok(resp) = reqwest::get(&a.url).await {
                    if let Ok(bytes) = resp.bytes().await {
                        parts.push(Part::blob(ct, bytes.to_vec()));
                        attachment_urls.push(a.url.clone());
                    }
                }
            }
        }

        contents.push(Content::from(parts));

        let mut response = None;
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
