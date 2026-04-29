use std::collections::VecDeque;

use super::models::ChatEntry;
use super::scheduler::{AdmissionConfig, AiOperation, AiScheduler};
use crate::configs::google::GOOGLE_CONFIGS;
use crate::services::ai::genai::{Auth, Client, Content, Part, Response};
use crate::services::ai::history::parse_history;
use async_trait::async_trait;
use tokio::sync::OnceCell;
use tokio::time::{Duration, sleep};

#[async_trait]
pub trait AiClient {
    async fn generate(
        &self,
        model: &str,
        system: &str,
        contents: Vec<Content>,
    ) -> anyhow::Result<Response>;
}

#[async_trait]
impl AiClient for Client {
    async fn generate(
        &self,
        model: &str,
        system: &str,
        contents: Vec<Content>,
    ) -> anyhow::Result<Response> {
        self.generative_model(model)
            .with_system_instruction(system)
            .generate_content(contents)
            .await
    }
}

const SYSTEM: &str = "You are a conversation summarizer. Given the chat history, produce a concise summary in English only, formatted as bullet points. Do NOT include any greetings, sign-offs, full sentences, or explanations—just the key facts.";

pub(super) static CLIENT: OnceCell<Client> = OnceCell::const_new();

#[derive(Clone, Copy)]
pub(super) struct ModelSpec {
    pub name: &'static str,
    pub rpm_limit: usize,
    pub queue_timeout: Duration,
    pub cooldown: Duration,
}

pub(super) const MODELS: &[ModelSpec] = &[
    ModelSpec {
        name: "gemini-2.5-flash",
        rpm_limit: 10,
        queue_timeout: Duration::from_secs(8),
        cooldown: Duration::from_secs(20),
    },
    ModelSpec {
        name: "gemini-2.5-flash-lite",
        rpm_limit: 15,
        queue_timeout: Duration::from_secs(8),
        cooldown: Duration::from_secs(15),
    },
    ModelSpec {
        name: "gemini-2.0-flash",
        rpm_limit: 15,
        queue_timeout: Duration::from_secs(8),
        cooldown: Duration::from_secs(12),
    },
    ModelSpec {
        name: "gemini-2.0-flash-lite",
        rpm_limit: 30,
        queue_timeout: Duration::from_secs(8),
        cooldown: Duration::from_secs(10),
    },
    ModelSpec {
        name: "gemini-2.5-flash-preview",
        rpm_limit: 10,
        queue_timeout: Duration::from_secs(8),
        cooldown: Duration::from_secs(20),
    },
    ModelSpec {
        name: "gemini-2.5-flash-lite-preview",
        rpm_limit: 15,
        queue_timeout: Duration::from_secs(8),
        cooldown: Duration::from_secs(15),
    },
];

pub(super) const SUMMARY_MODELS: &[ModelSpec] = &[
    ModelSpec {
        name: "gemini-2.5-flash-lite",
        rpm_limit: 15,
        queue_timeout: Duration::from_secs(5),
        cooldown: Duration::from_secs(15),
    },
    ModelSpec {
        name: "gemini-2.0-flash-lite",
        rpm_limit: 30,
        queue_timeout: Duration::from_secs(5),
        cooldown: Duration::from_secs(10),
    },
    ModelSpec {
        name: "gemini-2.0-flash",
        rpm_limit: 15,
        queue_timeout: Duration::from_secs(5),
        cooldown: Duration::from_secs(12),
    },
    ModelSpec {
        name: "gemini-2.5-flash",
        rpm_limit: 10,
        queue_timeout: Duration::from_secs(5),
        cooldown: Duration::from_secs(20),
    },
    ModelSpec {
        name: "gemini-2.5-flash-lite-preview",
        rpm_limit: 15,
        queue_timeout: Duration::from_secs(5),
        cooldown: Duration::from_secs(15),
    },
];

const RETRY_DELAYS_MS: &[u64] = &[250, 1000];

pub async fn client() -> anyhow::Result<&'static Client> {
    CLIENT
        .get_or_try_init(|| async {
            Client::new(Auth::ApiKey(GOOGLE_CONFIGS.api_key.clone()))
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
        .and_then(|p| p.text.clone())
        .unwrap_or_default()
}

pub(super) fn is_retryable(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_ascii_lowercase();
    [
        "resource exhausted",
        "rate limit",
        "too many requests",
        "deadline exceeded",
        "service unavailable",
        "transport failure",
        "connection reset",
        "temporarily unavailable",
        "timeout",
        "unavailable",
        "429",
        "500",
        "503",
    ]
    .iter()
    .any(|marker| msg.contains(marker))
}

pub(super) async fn generate_with_retries<C>(
    client: &C,
    model: &str,
    system: &str,
    contents: Vec<Content>,
) -> anyhow::Result<Response>
where
    C: AiClient + Send + Sync,
{
    let mut attempt = 0usize;

    loop {
        match client
            .generate(model, system, contents.clone())
            .await
        {
            Ok(resp) => return Ok(resp),
            Err(err) if attempt < RETRY_DELAYS_MS.len() && is_retryable(&err) => {
                let delay_ms = RETRY_DELAYS_MS[attempt];
                tracing::warn!(
                    model = %model,
                    attempt = attempt + 1,
                    delay_ms,
                    error = %err,
                    "transient model error; retrying",
                );
                sleep(Duration::from_millis(delay_ms)).await;
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

pub(super) async fn summarize<C>(
    client: &C,
    scheduler: &AiScheduler,
    history: &mut VecDeque<ChatEntry>,
    user_name: &str,
) -> anyhow::Result<String>
where
    C: AiClient + Send + Sync,
{
    let mut contents = parse_history(&*history, user_name).await;
    contents.push(Content::from(Part::text(SYSTEM)));

    for spec in SUMMARY_MODELS {
        let guard = scheduler
            .acquire(
                spec.name,
                AiOperation::Summary,
                AdmissionConfig { rpm_limit: spec.rpm_limit, queue_timeout: spec.queue_timeout },
            )
            .await;

        let guard = match guard {
            Ok(guard) => guard,
            Err(e) => {
                tracing::warn!(model = spec.name, error = %e, "summary model queue failed");
                continue;
            }
        };

        match generate_with_retries(client, spec.name, SYSTEM, contents.clone()).await {
            Ok(resp) => return Ok(extract_text(resp)),
            Err(e) => {
                if is_retryable(&e) {
                    guard.cool_down(spec.cooldown).await;
                }
                tracing::warn!(model = spec.name, error = %e, "summary model failed");
            }
        }
    }

    Err(anyhow::anyhow!("all models failed to summarize"))
}
