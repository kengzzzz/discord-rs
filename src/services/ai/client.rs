use std::collections::VecDeque;

use super::models::ChatEntry;
#[cfg(test)]
use super::tests::SUMMARIZE_OVERRIDE;
use crate::configs::google::GOOGLE_CONFIGS;
use crate::services::ai::history::parse_history;
use google_ai_rs::genai::Response;
use google_ai_rs::{Client, Content, Part};
use tokio::sync::OnceCell;

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

pub(super) async fn client() -> anyhow::Result<&'static Client> {
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

pub(super) async fn summarize(
    history: &mut VecDeque<ChatEntry>,
    user_name: &str,
) -> anyhow::Result<String> {
    #[cfg(test)]
    if let Some(f) = SUMMARIZE_OVERRIDE.get() {
        return Ok(f(history.make_contiguous()));
    }

    let client = client().await?;
    let mut contents = parse_history(&*history, user_name).await;
    contents.push(Content::from(Part::text(SYSTEM)));

    for name in SUMMARY_MODELS {
        let model = client
            .generative_model(name)
            .with_system_instruction(SYSTEM);
        match model.generate_content(contents.clone()).await {
            Ok(resp) => return Ok(extract_text(resp)),
            Err(e) => tracing::warn!(model = %name, error = %e, "summary model failed"),
        }
    }

    Err(anyhow::anyhow!("all models failed to summarize"))
}
