use super::models::ChatEntry;
#[cfg(test)]
use super::tests::SUMMARIZE_OVERRIDE;
use crate::configs::google::GOOGLE_CONFIGS;
use google_ai_rs::Client;
use google_ai_rs::{Content, Part, genai::Response};
use tokio::sync::OnceCell;

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

pub(super) async fn summarize(history: &[ChatEntry]) -> anyhow::Result<String> {
    #[cfg(test)]
    if let Some(f) = SUMMARIZE_OVERRIDE.get() {
        return Ok(f(history));
    }

    let client = client().await?;
    let contents: Vec<Content> = history
        .iter()
        .map(|c| {
            let mut parts = vec![Part::text(&c.text)];
            for url in &c.attachments {
                parts.push(Part::text("Attachment:"));
                parts.push(Part::file_data("", url));
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
                    let label = format!("Attachment from {owner}:");
                    parts.push(Part::text(&label));
                    parts.push(Part::file_data("", url));
                }
            }
            Content {
                role: c.role.clone(),
                parts,
            }
        })
        .collect();
    let system = "Summarize the conversation so far in a concise form. Include brief mentions of any attachments or quotes.";

    for name in SUMMARY_MODELS {
        let model = client
            .generative_model(name)
            .with_system_instruction(system);
        match model.generate_content(contents.clone()).await {
            Ok(resp) => return Ok(extract_text(resp)),
            Err(e) => tracing::warn!(model = %name, error = %e, "summary model failed"),
        }
    }

    Err(anyhow::anyhow!("all models failed to summarize"))
}
