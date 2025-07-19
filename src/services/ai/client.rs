use std::collections::VecDeque;

use super::models::ChatEntry;
use crate::configs::google::GOOGLE_CONFIGS;
use crate::services::ai::history::parse_history;
use async_trait::async_trait;
use google_ai_rs::genai::Response;
use google_ai_rs::{Client, Content, Part};
use tokio::sync::OnceCell;

#[async_trait]
pub trait AiClient {
    async fn generate(
        &self,
        model: &str,
        system: &str,
        contents: Vec<Content>,
    ) -> Result<Response, google_ai_rs::error::Error>;
}

#[async_trait]
impl AiClient for Client {
    async fn generate(
        &self,
        model: &str,
        system: &str,
        contents: Vec<Content>,
    ) -> Result<Response, google_ai_rs::error::Error> {
        self.generative_model(model)
            .with_system_instruction(system)
            .generate_content(contents)
            .await
    }
}

const SYSTEM: &str = "You are a conversation summarizer. Given the chat history, produce a concise summary in English only, formatted as bullet points. Do NOT include any greetings, sign-offs, full sentences, or explanations—just the key facts.";

pub(super) static CLIENT: OnceCell<Client> = OnceCell::const_new();

pub(super) const MODELS: &[&str] = &[
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite-preview-06-17",
    "gemini-2.5-flash-preview-tts",
    "gemini-2.0-flash",
    "gemini-2.0-flash-preview-image-generation",
    "gemini-2.0-flash-lite",
];

pub(super) const SUMMARY_MODELS: &[&str] = &[
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite-preview-06-17",
    "gemini-2.5-flash-preview-tts",
    "gemini-2.0-flash",
    "gemini-2.0-flash-lite",
];

pub async fn client() -> anyhow::Result<&'static Client> {
    CLIENT
        .get_or_try_init(|| async {
            Client::new(google_ai_rs::Auth::ApiKey(GOOGLE_CONFIGS.api_key.clone()))
                .await
                .map_err(anyhow::Error::msg)
        })
        .await
}

pub(super) fn extract_text(response: Response) -> String {
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

pub(super) async fn summarize<C>(
    client: &C,
    history: &mut VecDeque<ChatEntry>,
    user_name: &str,
) -> anyhow::Result<String>
where
    C: AiClient + Send + Sync,
{
    let mut contents = parse_history(&*history, user_name).await;
    contents.push(Content::from(Part::text(SYSTEM)));

    for name in SUMMARY_MODELS {
        match client.generate(name, SYSTEM, contents.clone()).await {
            Ok(resp) => return Ok(extract_text(resp)),
            Err(e) => tracing::warn!(model = %name, error = %e, "summary model failed"),
        }
    }

    Err(anyhow::anyhow!("all models failed to summarize"))
}
