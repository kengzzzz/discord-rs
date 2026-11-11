use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const OVERALL_DEADLINE: Duration = Duration::from_secs(75);
/// One slow model must not use the whole deadline; good answers come in about 10 s.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(15);
/// Also covers the upload, which can take most of 15 s for 15 MiB.
const LONG_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(45);
/// Longer input gets the long timeout and never parks a model.
const LONG_INPUT_CHARS: usize = 1500;
const SLOW_PARK: Duration = Duration::from_secs(10 * 60);
const OVERLOAD_PARK: Duration = Duration::from_secs(5 * 60);
const MAX_RETRY_SLEEP: Duration = Duration::from_secs(10);
/// A max-length reply is a few hundred KB.
const MAX_BODY_BYTES: usize = 1 << 20;

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    /// Bot-added text (search results); doesn't count as long input.
    Context(String),
    Inline {
        mime: String,
        data: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effort {
    Minimal,
    Low,
}

#[derive(Clone, Copy)]
struct Call {
    deadline: Instant,
    long: bool,
    id: u64,
}

pub struct Request<'a> {
    pub system: &'a str,
    pub history: &'a [(bool, String)],
    pub parts: Vec<Part>,
    pub effort: Effort,
    pub grounding: bool,
    /// Rejects a successful reply so the next model is tried instead.
    pub accept: Option<fn(&str) -> bool>,
    /// One response, so a long answer can outlast the attempt timeout.
    pub max_output_tokens: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    pub model: String,
    pub truncated: bool,
    pub sources: Vec<Source>,
    pub queries: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub title: String,
    pub uri: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AiError {
    #[error("AI is not configured")]
    NotConfigured,
    #[error("request blocked: {0}")]
    Blocked(String),
    #[error("response resembles copyrighted text")]
    Recitation,
    #[error("all models failed: {0}")]
    Unavailable(String),
    #[error("every model rejected the request: {0}")]
    Rejected(String),
    #[error("every model returned an untranslated reply")]
    Untranslated,
}

pub struct Gemini {
    http: reqwest::Client,
    key: Option<String>,
    base: String,
    unavailable_until: Mutex<HashMap<String, Instant>>,
    /// A model is parked only on a second timeout from a different request.
    slow_strikes: Mutex<HashMap<String, (Instant, u64)>>,
    requests: std::sync::atomic::AtomicU64,
}

const UPLOAD_CHUNK: usize = 64 * 1024;

/// Hyper keeps uploading a body it owns after an early answer, so the attempt keeps the buffer
/// and these pieces end when it drops.
fn pieces(body: &Arc<Vec<u8>>) -> impl Iterator<Item = Result<Vec<u8>, &'static str>> + use<> {
    let (held, length) = (Arc::downgrade(body), body.len());
    (0..length)
        .step_by(UPLOAD_CHUNK)
        .map(move |start| {
            let body = held
                .upgrade()
                .ok_or("the attempt ended")?;
            Ok(body[start..length.min(start + UPLOAD_CHUNK)].to_vec())
        })
}

enum Outcome {
    Done(Reply),
    Fatal(AiError),
    RetrySame(Duration),
    NextModel {
        exhausted_for: Option<Duration>,
        reason: String,
    },
    /// A 400. Not parked: one bad file must not disable the model for everyone.
    Rejected(String),
    DropGrounding,
}

impl Gemini {
    pub fn new(http: reqwest::Client, key: Option<String>) -> Self {
        Self {
            http,
            key,
            base: BASE.to_owned(),
            unavailable_until: Mutex::new(HashMap::new()),
            slow_strikes: Mutex::new(HashMap::new()),
            requests: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn configured(&self) -> bool {
        self.key.is_some()
    }

    /// `grounding` overrides the request's, so a fallback can drop it without copying files.
    pub async fn generate(
        &self,
        models: &[String],
        request: &Request<'_>,
        grounding: bool,
    ) -> Result<Reply, AiError> {
        let key = self
            .key
            .as_deref()
            .ok_or(AiError::NotConfigured)?;
        let deadline = Instant::now() + OVERALL_DEADLINE;
        let mut last_error = String::from("no usable model configured");
        let (mut tried, mut rejected, mut unaccepted) = (0usize, 0usize, 0usize);
        let input_chars: usize = request
            .parts
            .iter()
            .map(|p| match p {
                Part::Text(text) => text.chars().count(),
                Part::Context(_) | Part::Inline { .. } => 0,
            })
            .sum();
        let long = input_chars > LONG_INPUT_CHARS
            || request
                .parts
                .iter()
                .any(|p| matches!(p, Part::Inline { .. }));
        let request_id = self
            .requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let call = Call { deadline, long, id: request_id };

        for model in models {
            if self.is_unavailable(model) {
                continue;
            }
            tried += 1;
            let mut grounding = grounding;
            let mut retried = false;
            loop {
                if Instant::now() >= deadline {
                    return Err(AiError::Unavailable("deadline exceeded".into()));
                }
                let body = request_body(model, request, grounding);
                match self
                    .attempt(key, model, body, grounding, call)
                    .await
                {
                    Outcome::Done(reply)
                        if request
                            .accept
                            .is_some_and(|accept| !accept(&reply.text)) =>
                    {
                        metrics::counter!("ai_rejected_total", "model" => model.to_owned())
                            .increment(1);
                        tracing::warn!(model, "reply rejected; trying next model");
                        last_error = format!("{model}: reply rejected");
                        unaccepted += 1;
                        break;
                    }
                    Outcome::Done(reply) => return Ok(reply),
                    Outcome::Fatal(error) => return Err(error),
                    Outcome::DropGrounding if grounding => grounding = false,
                    Outcome::DropGrounding => break,
                    Outcome::RetrySame(delay) if !retried => {
                        retried = true;
                        tokio::time::sleep(delay.min(MAX_RETRY_SLEEP)).await;
                    }
                    Outcome::RetrySame(_) => {
                        last_error = format!("{model}: retry failed");
                        break;
                    }
                    Outcome::NextModel { exhausted_for, reason } => {
                        if let Some(duration) = exhausted_for {
                            self.mark_unavailable(model, duration);
                        }
                        tracing::warn!(model, reason, "gemini model failed; trying next");
                        last_error = format!("{model}: {reason}");
                        break;
                    }
                    Outcome::Rejected(reason) => {
                        tracing::warn!(
                            model,
                            reason,
                            "request rejected; trying next model"
                        );
                        last_error = format!("{model}: {reason}");
                        rejected += 1;
                        break;
                    }
                }
            }
        }
        if tried > 0 && rejected == tried {
            return Err(AiError::Rejected(last_error));
        }
        if tried > 0 && unaccepted == tried {
            return Err(AiError::Untranslated);
        }
        Err(AiError::Unavailable(last_error))
    }

    async fn attempt(
        &self,
        key: &str,
        model: &str,
        body: Vec<u8>,
        grounded: bool,
        call: Call,
    ) -> Outcome {
        let Call { deadline, long, id: request_id } = call;
        let limit = if long { LONG_ATTEMPT_TIMEOUT } else { ATTEMPT_TIMEOUT };
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .min(limit);
        // Parking affects everyone, so only a full timeout on a short request counts.
        let may_park = !long && timeout >= ATTEMPT_TIMEOUT;
        let park = || {
            may_park
                .then(|| self.strike(model, request_id))
                .flatten()
        };
        let request = self
            .http
            .post(format!("{}/{model}:generateContent", self.base))
            .header("x-goog-api-key", key)
            .timeout(timeout)
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        let (request, _held) = if body.len() > UPLOAD_CHUNK {
            let held = Arc::new(body);
            let stream = futures::stream::iter(pieces(&held));
            (
                request
                    .header(reqwest::header::CONTENT_LENGTH, held.len())
                    .body(reqwest::Body::wrap_stream(stream)),
                Some(held),
            )
        } else {
            (request.body(body), None)
        };
        let response = request.send().await;
        let response = match response {
            Ok(response) => response,
            Err(error) if error.is_timeout() => {
                metrics::counter!("ai_requests_total", "model" => model.to_owned(), "status" => "timeout")
                    .increment(1);
                return Outcome::NextModel {
                    // Never reaching the server says nothing about the model.
                    exhausted_for: if error.is_connect() { None } else { park() },
                    reason: format!("no answer within {}s", timeout.as_secs()),
                };
            }
            Err(error) => {
                metrics::counter!("ai_requests_total", "model" => model.to_owned(), "status" => "transport")
                    .increment(1);
                return Outcome::NextModel {
                    exhausted_for: None,
                    reason: format!("transport: {error}"),
                };
            }
        };
        let status = response.status().as_u16();
        let bytes = match crate::warframe::read_capped(response, MAX_BODY_BYTES).await {
            Ok(bytes) => bytes,
            // Headers arrived but the body stalled: as slow as no answer at all.
            Err(error)
                if error
                    .downcast_ref::<reqwest::Error>()
                    .is_some_and(reqwest::Error::is_timeout) =>
            {
                metrics::counter!("ai_requests_total", "model" => model.to_owned(), "status" => "timeout")
                    .increment(1);
                return Outcome::NextModel {
                    exhausted_for: park(),
                    reason: format!(
                        "response body stalled past {}s",
                        timeout.as_secs()
                    ),
                };
            }
            Err(error) => {
                return match error.downcast::<reqwest::Error>() {
                    Ok(error) => {
                        metrics::counter!("ai_requests_total", "model" => model.to_owned(), "status" => status.to_string())
                            .increment(1);
                        Outcome::RetrySame(Duration::from_secs(1)).or_log(error)
                    }
                    Err(error) => {
                        metrics::counter!("ai_requests_total", "model" => model.to_owned(), "status" => "too_large")
                            .increment(1);
                        tracing::warn!(model, status, %error, "gemini response body over the cap");
                        // Every model would answer as large, so stop here.
                        if (200..300).contains(&status) {
                            Outcome::Fatal(AiError::Unavailable(error.to_string()))
                        } else {
                            Outcome::NextModel { exhausted_for: None, reason: error.to_string() }
                        }
                    }
                };
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        metrics::counter!("ai_requests_total", "model" => model.to_owned(), "status" => status.to_string())
            .increment(1);
        classify(model, status, &text, grounded)
    }

    fn is_unavailable(&self, model: &str) -> bool {
        let mut map = self
            .unavailable_until
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match map.get(model) {
            Some(until) if *until > Instant::now() => true,
            Some(_) => {
                map.remove(model);
                false
            }
            None => false,
        }
    }

    fn strike(&self, model: &str, request_id: u64) -> Option<Duration> {
        let mut strikes = self
            .slow_strikes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        match strikes.get(model) {
            Some((at, id)) if *id != request_id && now.duration_since(*at) < SLOW_PARK => {
                strikes.remove(model);
                Some(SLOW_PARK)
            }
            _ => {
                strikes.insert(model.to_owned(), (now, request_id));
                None
            }
        }
    }

    fn mark_unavailable(&self, model: &str, duration: Duration) {
        self.unavailable_until
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(model.to_owned(), Instant::now() + duration);
    }
}

trait OrLog {
    fn or_log(self, error: reqwest::Error) -> Self;
}

impl OrLog for Outcome {
    fn or_log(self, error: reqwest::Error) -> Self {
        tracing::warn!(%error, "failed to read gemini response body");
        self
    }
}

fn thinking_config(model: &str, effort: Effort) -> Value {
    if model.starts_with("gemma-") {
        // Gemma has only MINIMAL and HIGH, and HIGH outlasts the typing indicator.
        return json!({ "thinkingLevel": "MINIMAL" });
    }
    if model.starts_with("gemini-2.") {
        let budget = if model.contains("lite") { 0 } else { 1024 };
        return json!({ "thinkingBudget": budget });
    }
    let level = match effort {
        Effort::Minimal if model.starts_with("gemini-3.8") || model.starts_with("gemini-4") => {
            "LOW"
        }
        Effort::Minimal => "MINIMAL",
        Effort::Low => "LOW",
    };
    json!({ "thinkingLevel": level })
}

// Fields in alphabetical order, as `json!` wrote them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Body<'a> {
    contents: Vec<Turn<'a>>,
    generation_config: GenerationConfig,
    system_instruction: Turn<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Value>,
}

#[derive(Serialize)]
struct Turn<'a> {
    parts: Vec<BodyPart<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum BodyPart<'a> {
    Text(&'a str),
    InlineData(InlineData<'a>),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InlineData<'a> {
    data: Base64<'a>,
    mime_type: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    max_output_tokens: u32,
    thinking_config: Value,
}

/// Encoded straight into the body, so a 15 MiB file is never also held as a base64 `String`.
struct Base64<'a>(&'a [u8]);

impl Serialize for Base64<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&base64::display::Base64Display::new(
            self.0,
            &base64::engine::general_purpose::STANDARD,
        ))
    }
}

/// Built in one buffer sized up front, so it doesn't regrow while holding 20 MiB of base64.
pub fn request_body(model: &str, request: &Request<'_>, grounding: bool) -> Vec<u8> {
    let has_text = request
        .parts
        .iter()
        .any(|p| matches!(p, Part::Text(t) | Part::Context(t) if !t.trim().is_empty()));
    let mut parts: Vec<BodyPart<'_>> = request
        .parts
        .iter()
        .map(|part| match part {
            Part::Text(text) | Part::Context(text) => BodyPart::Text(text),
            Part::Inline { mime, data } => {
                BodyPart::InlineData(InlineData { data: Base64(data), mime_type: mime })
            }
        })
        .collect();
    if !has_text {
        parts.push(BodyPart::Text("(no text)"));
    }
    let mut contents: Vec<Turn<'_>> = request
        .history
        .iter()
        .map(|(from_model, text)| Turn {
            parts: vec![BodyPart::Text(text)],
            role: Some(if *from_model { "model" } else { "user" }),
        })
        .collect();
    contents.push(Turn { parts, role: Some("user") });
    let body = Body {
        contents,
        generation_config: GenerationConfig {
            max_output_tokens: request.max_output_tokens,
            thinking_config: thinking_config(model, request.effort),
        },
        system_instruction: Turn { parts: vec![BodyPart::Text(request.system)], role: None },
        tools: grounding.then(|| json!([{ "googleSearch": {} }])),
    };

    let (mut files, mut text) = (0, request.system.len());
    for part in &request.parts {
        match part {
            Part::Text(t) | Part::Context(t) => text += t.len(),
            Part::Inline { data, .. } => files += data.len().div_ceil(3) * 4,
        }
    }
    text += request
        .history
        .iter()
        .map(|(_, t)| t.len())
        .sum::<usize>();
    // Escapes at most double text. Rare inputs exceed this; the buffer then grows once.
    let mut out = Vec::with_capacity(files + 2 * text + 4096);
    serde_json::to_writer(&mut out, &body).expect("writing JSON to a Vec cannot fail");
    out
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Response {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(default)]
    prompt_feedback: Option<PromptFeedback>,
    #[serde(default)]
    model_version: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PromptFeedback {
    #[serde(default)]
    block_reason: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    #[serde(default)]
    content: Option<Content>,
    #[serde(default)]
    finish_reason: Option<String>,
    #[serde(default)]
    grounding_metadata: Option<GroundingMetadata>,
}

#[derive(Deserialize, Default)]
struct Content {
    #[serde(default)]
    parts: Vec<ResponsePart>,
}

#[derive(Deserialize, Default)]
struct ResponsePart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thought: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GroundingMetadata {
    #[serde(default)]
    web_search_queries: Vec<String>,
    #[serde(default)]
    grounding_chunks: Vec<GroundingChunk>,
}

#[derive(Deserialize, Default)]
struct GroundingChunk {
    #[serde(default)]
    web: Option<WebChunk>,
}

#[derive(Deserialize, Default)]
struct WebChunk {
    #[serde(default)]
    uri: String,
    #[serde(default)]
    title: String,
}

#[derive(Deserialize, Serialize, Default)]
struct ErrorEnvelope {
    #[serde(default)]
    error: Option<ErrorBody>,
}

#[derive(Deserialize, Serialize, Default)]
struct ErrorBody {
    #[serde(default)]
    message: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    details: Vec<Value>,
}

fn classify(model: &str, status: u16, body: &str, grounded: bool) -> Outcome {
    match status {
        200 => parse_success(model, body),
        429 => {
            let error = serde_json::from_str::<ErrorEnvelope>(body)
                .ok()
                .and_then(|e| e.error)
                .unwrap_or_default();
            let daily = error.details.iter().any(|d| {
                let violations = d
                    .get("violations")
                    .and_then(Value::as_array);
                violations.is_some_and(|vs| {
                    vs.iter().any(|v| {
                        let id = v
                            .get("quotaId")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let value = v.get("quotaValue").map(|q| {
                            q.to_string()
                                .trim_matches('"')
                                .to_owned()
                        });
                        id.contains("PerDay") || value.as_deref() == Some("0")
                    })
                })
            }) || error.message.contains("limit: 0");
            let search = error
                .message
                .to_ascii_lowercase()
                .contains("search")
                || error
                    .details
                    .iter()
                    .any(|d| d.to_string().contains("search"));
            if grounded && search {
                return Outcome::DropGrounding;
            }
            if daily {
                return Outcome::NextModel {
                    exhausted_for: Some(until_pacific_midnight()),
                    reason: "daily quota exhausted".into(),
                };
            }
            match retry_delay(&error.details) {
                // Retrying would hit the same limit, so park for the delay Google asked for.
                Some(delay) if delay > MAX_RETRY_SLEEP => Outcome::NextModel {
                    exhausted_for: Some(delay),
                    reason: format!("rate limited for {}s", delay.as_secs()),
                },
                delay => Outcome::RetrySame(delay.unwrap_or(Duration::from_secs(2))),
            }
        }
        400 if grounded => Outcome::DropGrounding,
        400 => Outcome::Rejected(format!(
            "rejected request: {}",
            error_message(body)
        )),
        403 => Outcome::NextModel {
            exhausted_for: Some(Duration::from_secs(60 * 60)),
            reason: format!("permission denied: {}", error_message(body)),
        },
        404 => Outcome::NextModel {
            exhausted_for: Some(Duration::from_secs(6 * 60 * 60)),
            reason: "model not found".into(),
        },
        503 => Outcome::NextModel {
            exhausted_for: Some(OVERLOAD_PARK),
            reason: format!("overloaded: {}", error_message(body)),
        },
        // Usually this request was too large, not a model outage.
        504 => Outcome::NextModel {
            exhausted_for: None,
            reason: format!("deadline exceeded: {}", error_message(body)),
        },
        500 => Outcome::NextModel { exhausted_for: None, reason: "internal error".into() },
        _ => {
            let error = serde_json::from_str::<ErrorEnvelope>(body)
                .ok()
                .and_then(|e| e.error)
                .unwrap_or_default();
            Outcome::NextModel {
                exhausted_for: None,
                reason: format!("{status} {}: {}", error.status, error.message),
            }
        }
    }
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<ErrorEnvelope>(body)
        .ok()
        .and_then(|e| e.error)
        .map(|e| e.message)
        .unwrap_or_default()
}

fn parse_success(model: &str, body: &str) -> Outcome {
    let response: Response = match serde_json::from_str(body) {
        Ok(response) => response,
        Err(error) => {
            return Outcome::NextModel { exhausted_for: None, reason: format!("decode: {error}") };
        }
    };
    if let Some(reason) = response
        .prompt_feedback
        .and_then(|f| f.block_reason)
    {
        return Outcome::Fatal(AiError::Blocked(reason));
    }
    let Some(candidate) = response.candidates.into_iter().next() else {
        return Outcome::RetrySame(Duration::from_secs(1));
    };
    let finish = candidate
        .finish_reason
        .unwrap_or_default();
    match finish.as_str() {
        "SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "SPII" | "IMAGE_SAFETY" => {
            return Outcome::Fatal(AiError::Blocked(finish));
        }
        "RECITATION" => return Outcome::Fatal(AiError::Recitation),
        _ => {}
    }
    let text: String = candidate
        .content
        .map(|c| {
            c.parts
                .into_iter()
                .filter(|p| !p.thought)
                .filter_map(|p| p.text)
                .collect()
        })
        .unwrap_or_default();
    // Control characters show as nothing in Discord and take 6 bytes each in a JSON body.
    let text: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    if text.is_empty() {
        return Outcome::NextModel {
            exhausted_for: None,
            reason: format!("empty response ({finish})"),
        };
    }
    let grounding = candidate
        .grounding_metadata
        .unwrap_or_default();
    let mut sources: Vec<Source> = Vec::new();
    for chunk in grounding
        .grounding_chunks
        .into_iter()
        .filter_map(|c| c.web)
    {
        if !chunk.uri.is_empty()
            && !sources
                .iter()
                .any(|s| s.uri == chunk.uri)
        {
            sources.push(Source { title: chunk.title, uri: chunk.uri });
        }
    }
    Outcome::Done(Reply {
        text,
        model: response
            .model_version
            .unwrap_or_else(|| model.to_owned()),
        truncated: finish == "MAX_TOKENS",
        sources,
        queries: grounding.web_search_queries,
    })
}

fn retry_delay(details: &[Value]) -> Option<Duration> {
    details.iter().find_map(|d| {
        let raw = d.get("retryDelay")?.as_str()?;
        raw.trim_end_matches('s')
            .parse::<f64>()
            .ok()
            .and_then(|secs| Duration::try_from_secs_f64(secs).ok())
            // A huge value would overflow `Instant::now() + delay` when parked.
            .map(|delay| delay.min(Duration::from_secs(24 * 60 * 60)))
    })
}

fn until_pacific_midnight() -> Duration {
    until_pacific_midnight_from(jiff::Timestamp::now())
}

/// Quotas reset at midnight Pacific. Uses bundled tz data: the image ships only Asia/Bangkok.
fn until_pacific_midnight_from(now: jiff::Timestamp) -> Duration {
    let next = jiff::tz::TimeZoneDatabase::bundled()
        .get("America/Los_Angeles")
        .ok()
        .and_then(|tz| {
            now.to_zoned(tz.clone())
                .date()
                .tomorrow()
                .ok()?
                .to_zoned(tz)
                .ok()
        })
        .map_or_else(
            || {
                let (reset, day) = (8 * 3600, 86_400);
                (now.as_second() - reset).div_euclid(day) * day + day + reset
            },
            |midnight| midnight.timestamp().as_second(),
        );
    Duration::from_secs((next - now.as_second()).max(60) as u64)
}

/// PNG re-encoding isn't capped by area, so sides and area are limited here.
const MAX_CONVERT_SIDE: u32 = 4096;
const MAX_CONVERT_PIXELS: u64 = 2048 * 2048;
const MAX_CONVERT_ALLOC: u64 = 32 * 1024 * 1024;

/// A tiny GIF can declare 11000x11000 pixels (about 460 MiB decoded). Runs in the image worker.
fn decode_bounded(
    bytes: &[u8],
    format: image::ImageFormat,
) -> image::ImageResult<image::DynamicImage> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_CONVERT_SIDE);
    limits.max_image_height = Some(MAX_CONVERT_SIDE);
    limits.max_alloc = Some(MAX_CONVERT_ALLOC);
    let reader = || {
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
        reader.limits(limits.clone());
        reader
    };
    let (width, height) = reader().into_dimensions()?;
    if u64::from(width) * u64::from(height) > MAX_CONVERT_PIXELS
        || !crate::moderation::ocr::gif_frames_fit(bytes, MAX_CONVERT_PIXELS)
    {
        return Err(image::ImageError::Limits(
            image::error::LimitError::from_kind(image::error::LimitErrorKind::DimensionError),
        ));
    }
    reader().decode()
}

pub fn supported_mime(mime: &str) -> bool {
    matches!(
        mime,
        "image/png"
            | "image/jpeg"
            | "image/webp"
            | "image/heic"
            | "image/heif"
            | "application/pdf"
            | "image/gif"
            | "image/bmp"
    )
}

fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    }
}

fn is_image_label(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "image/bmp"
    )
}

pub fn needs_conversion(mime: &str, bytes: &[u8]) -> Option<&'static str> {
    if !is_image_label(mime) {
        return None;
    }
    sniff(bytes).filter(|real| matches!(*real, "image/gif" | "image/bmp"))
}

/// Gemini rejects inline data whose bytes don't match its MIME type.
pub fn normalize_image(mime: &str, bytes: Vec<u8>) -> Option<(String, Vec<u8>)> {
    match mime {
        "image/heic" | "image/heif" | "application/pdf" => Some((mime.to_owned(), bytes)),
        mime if is_image_label(mime) => match sniff(&bytes)? {
            real @ ("image/gif" | "image/bmp") => {
                let format = if real == "image/gif" {
                    image::ImageFormat::Gif
                } else {
                    image::ImageFormat::Bmp
                };
                let decoded = decode_bounded(&bytes, format).ok()?;
                let mut out = std::io::Cursor::new(Vec::new());
                decoded
                    .write_to(&mut out, image::ImageFormat::Png)
                    .ok()?;
                Some(("image/png".to_owned(), out.into_inner()))
            }
            real => Some((real.to_owned(), bytes)),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    fn body_json(model: &str, request: &Request<'_>, grounding: bool) -> Value {
        serde_json::from_slice(&request_body(model, request, grounding)).unwrap()
    }

    #[test]
    fn request_body_matches_the_json_value_form() {
        let history = [(false, "hi \"there\"\n".to_owned()), (true, "สวัสดี".to_owned())];
        let data: Vec<u8> = (0..=255u8).cycle().take(3001).collect();
        let req = Request {
            system: "sys",
            history: &history,
            parts: vec![
                Part::Context("ctx".into()),
                Part::Inline { mime: "image/png".into(), data: data.clone() },
                Part::Text("q".into()),
            ],
            effort: Effort::Low,
            grounding: true,
            max_output_tokens: 2048,
            accept: None,
        };
        let expected = json!({
            "systemInstruction": { "parts": [{ "text": "sys" }] },
            "contents": [
                { "role": "user", "parts": [{ "text": "hi \"there\"\n" }] },
                { "role": "model", "parts": [{ "text": "สวัสดี" }] },
                { "role": "user", "parts": [
                    { "text": "ctx" },
                    { "inlineData": {
                        "mimeType": "image/png",
                        "data": base64::engine::general_purpose::STANDARD.encode(&data),
                    } },
                    { "text": "q" },
                ] },
            ],
            "generationConfig": {
                "maxOutputTokens": 2048,
                "thinkingConfig": thinking_config("gemini-3.5-flash", Effort::Low),
            },
            "tools": [{ "googleSearch": {} }],
        });
        let bytes = request_body("gemini-3.5-flash", &req, true);
        assert_eq!(bytes, serde_json::to_vec(&expected).unwrap());
        let plain = request_body("gemini-3.5-flash", &req, false);
        assert!(
            serde_json::from_slice::<Value>(&plain)
                .unwrap()
                .get("tools")
                .is_none()
        );
    }

    fn request(parts: Vec<Part>) -> Request<'static> {
        Request {
            system: "sys",
            history: &[],
            parts,
            effort: Effort::Minimal,
            grounding: false,
            max_output_tokens: 8192,
            accept: None,
        }
    }

    #[test]
    fn thinking_knob_follows_model_family() {
        assert_eq!(
            thinking_config("gemini-3.5-flash-lite", Effort::Minimal),
            json!({"thinkingLevel": "MINIMAL"})
        );
        assert_eq!(
            thinking_config("gemini-3.8-flash", Effort::Minimal),
            json!({"thinkingLevel": "LOW"})
        );
        assert_eq!(
            thinking_config("gemini-2.5-flash-lite", Effort::Low),
            json!({"thinkingBudget": 0})
        );
        assert_eq!(
            thinking_config("gemma-4-31b-it", Effort::Minimal),
            json!({"thinkingLevel": "MINIMAL"})
        );
        assert_eq!(
            thinking_config("gemma-4-26b-a4b-it", Effort::Low),
            json!({"thinkingLevel": "MINIMAL"})
        );
    }

    #[test]
    fn image_only_request_gets_a_text_part_after_the_image() {
        let body = body_json(
            "gemini-3.5-flash-lite",
            &request(vec![Part::Inline {
                mime: "image/png".into(),
                data: vec![1, 2, 3],
            }]),
            false,
        );
        let parts = body["contents"][0]["parts"]
            .as_array()
            .unwrap();
        assert_eq!(parts[0]["inlineData"]["data"], "AQID");
        assert_eq!(parts[1]["text"], "(no text)");
        assert!(body.get("tools").is_none());
        assert!(
            body["generationConfig"]
                .get("temperature")
                .is_none()
        );
    }

    #[test]
    fn history_alternates_and_grounding_adds_tool() {
        let history = [(false, "hi".to_owned()), (true, "hello".to_owned())];
        let req = Request {
            system: "s",
            history: &history,
            parts: vec![Part::Text("q".into())],
            effort: Effort::Low,
            grounding: true,
            max_output_tokens: 8192,
            accept: None,
        };
        let body = body_json("gemini-2.5-flash-lite", &req, true);
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][1]["role"], "model");
        assert_eq!(body["contents"][2]["role"], "user");
        assert_eq!(body["tools"][0], json!({"googleSearch": {}}));
    }

    #[test]
    fn success_joins_text_parts_skips_thoughts_and_collects_sources() {
        let body = json!({
            "candidates": [{
                "content": {"parts": [{"text": "think", "thought": true}, {"text": "สวัสดี "}, {"text": "ครับ"}]},
                "finishReason": "STOP",
                "groundingMetadata": {
                    "webSearchQueries": ["warframe hate incarnon"],
                    "groundingChunks": [{"web": {"uri": "https://a", "title": "A"}}, {"web": {"uri": "https://a", "title": "A"}}]
                }
            }],
            "modelVersion": "gemini-3.5-flash-lite"
        })
        .to_string();
        let Outcome::Done(reply) = classify("m", 200, &body, true) else {
            panic!("expected reply")
        };
        assert_eq!(reply.text, "สวัสดี ครับ");
        assert_eq!(reply.sources.len(), 1);
        assert_eq!(reply.queries, ["warframe hate incarnon"]);
        assert!(!reply.truncated);
    }

    #[test]
    fn control_characters_are_dropped_from_replies() {
        let body = json!({
            "candidates": [{"content": {"parts": [{"text": "a\u{1}b\n\tc\u{1b}\u{85}"}]}, "finishReason": "STOP"}]
        })
        .to_string();
        let Outcome::Done(reply) = classify("m", 200, &body, true) else {
            panic!("expected reply")
        };
        assert_eq!(reply.text, "ab\n\tc");
    }

    #[test]
    fn blocked_and_recitation_are_fatal() {
        let blocked = json!({"promptFeedback": {"blockReason": "SAFETY"}}).to_string();
        assert!(matches!(
            classify("m", 200, &blocked, false),
            Outcome::Fatal(AiError::Blocked(_))
        ));
        let recited = json!({"candidates": [{"finishReason": "RECITATION"}]}).to_string();
        assert!(matches!(
            classify("m", 200, &recited, false),
            Outcome::Fatal(AiError::Recitation)
        ));
    }

    #[test]
    fn daily_quota_429_moves_on_and_minute_quota_retries() {
        let daily = json!({"error": {"code": 429, "status": "RESOURCE_EXHAUSTED", "message": "quota", "details": [
            {"@type": "type.googleapis.com/google.rpc.QuotaFailure", "violations": [{"quotaId": "GenerateRequestsPerDayPerProjectPerModel-FreeTier", "quotaValue": "20"}]},
            {"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "3600s"}
        ]}}).to_string();
        assert!(matches!(
            classify("m", 429, &daily, false),
            Outcome::NextModel { exhausted_for: Some(_), .. }
        ));

        let minute = json!({"error": {"code": 429, "status": "RESOURCE_EXHAUSTED", "message": "quota", "details": [
            {"@type": "type.googleapis.com/google.rpc.QuotaFailure", "violations": [{"quotaId": "GenerateRequestsPerMinutePerProjectPerModel-FreeTier", "quotaValue": 5}]},
            {"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "2.5s"}
        ]}}).to_string();
        let Outcome::RetrySame(delay) = classify("m", 429, &minute, false) else {
            panic!("expected retry")
        };
        assert_eq!(delay, Duration::from_millis(2500));
        // A delay too large to add to the clock is capped instead of panicking when parked.
        assert_eq!(
            retry_delay(&[json!({"retryDelay": "1e18s"})]),
            Some(Duration::from_secs(24 * 60 * 60))
        );
        assert_eq!(retry_delay(&[json!({"retryDelay": "-5s"})]), None);
    }

    #[test]
    fn rejected_requests_do_not_park_models_and_thinking_gets_room() {
        let bad = json!({"error": {"code": 400, "status": "INVALID_ARGUMENT", "message": "inline data not supported"}}).to_string();
        assert!(matches!(
            classify("gemma-4-31b-it", 400, &bad, false),
            Outcome::Rejected(_)
        ));
        let mut req = request(vec![Part::Text("q".into())]);
        req.effort = Effort::Low;
        assert_eq!(
            body_json("gemma-4-31b-it", &req, false)["generationConfig"]["maxOutputTokens"],
            8192
        );
    }

    #[test]
    fn grounded_failures_drop_the_search_tool() {
        let search =
            json!({"error": {"code": 429, "message": "search quota limit: 0", "details": []}})
                .to_string();
        assert!(matches!(
            classify("m", 429, &search, true),
            Outcome::DropGrounding
        ));
        assert!(matches!(
            classify("m", 400, "{}", true),
            Outcome::DropGrounding
        ));
    }

    #[test]
    fn gif_is_converted_to_png() {
        let mut gif = Vec::new();
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(
                &mut std::io::Cursor::new(&mut gif),
                image::ImageFormat::Gif,
            )
            .unwrap();
        let (mime, png) = normalize_image("image/gif", gif).unwrap();
        assert_eq!(mime, "image/png");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(normalize_image("application/zip", vec![1]).is_none());
    }

    #[test]
    fn quota_parking_ends_at_pacific_midnight_in_both_seasons() {
        assert!(
            jiff::tz::TimeZoneDatabase::bundled()
                .get("America/Los_Angeles")
                .is_ok()
        );
        let at = |s: &str| s.parse::<jiff::Timestamp>().unwrap();
        // 23:30 PDT on 30 June and 23:30 PST on 14 January.
        assert_eq!(
            until_pacific_midnight_from(at("2026-07-01T06:30:00Z")),
            Duration::from_secs(1800)
        );
        assert_eq!(
            until_pacific_midnight_from(at("2026-01-15T07:30:00Z")),
            Duration::from_secs(1800)
        );
        // Just after midnight PDT: almost a full day.
        assert_eq!(
            until_pacific_midnight_from(at("2026-07-01T07:00:01Z")),
            Duration::from_secs(86_399)
        );
    }

    #[test]
    fn one_slow_request_does_not_park_a_model() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let gemini = Gemini::new(reqwest::Client::new(), None);
        assert_eq!(gemini.strike("m", 1), None);
        // The same request timing out again (a retry) is still one strike.
        assert_eq!(gemini.strike("m", 1), None);
        assert_eq!(gemini.strike("other", 2), None);
        assert_eq!(gemini.strike("m", 2), Some(SLOW_PARK));
        assert_eq!(gemini.strike("m", 3), None);
    }

    fn answer_once(listener: std::net::TcpListener) -> std::thread::JoinHandle<Vec<u8>> {
        use std::io::{Read as _, Write as _};
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                request.extend_from_slice(&buf[..n]);
                if let Some(end) = request
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                {
                    let length = String::from_utf8_lossy(&request[..end])
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let body =
                r#"{"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"STOP"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            request
        })
    }

    #[tokio::test]
    async fn a_successful_attempt_is_not_a_strike() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = answer_once(listener);
        let mut gemini = Gemini::new(reqwest::Client::new(), Some("k".into()));
        gemini.base = format!("http://{addr}");
        let body = request_body("m", &request(vec![Part::Text("q".into())]), false);
        let deadline = Instant::now() + OVERALL_DEADLINE;
        let outcome = gemini
            .attempt(
                "k",
                "m",
                body,
                false,
                Call { deadline, long: false, id: 1 },
            )
            .await;
        assert!(matches!(outcome, Outcome::Done(_)));
        server.join().unwrap();
        // Request 1 never timed out, so request 2's first timeout is only a first strike.
        assert_eq!(gemini.strike("m", 2), None);
    }

    #[tokio::test]
    async fn a_large_body_is_sent_whole_from_a_buffer_the_attempt_keeps() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = answer_once(listener);
        let mut gemini = Gemini::new(reqwest::Client::new(), Some("k".into()));
        gemini.base = format!("http://{addr}");
        let file = Part::Inline {
            mime: "image/png".into(),
            data: (0..200_001u32)
                .map(|i| i as u8)
                .collect(),
        };
        let body = request_body(
            "m",
            &request(vec![file, Part::Text("q".into())]),
            false,
        );
        assert!(body.len() > 4 * UPLOAD_CHUNK && !body.len().is_multiple_of(UPLOAD_CHUNK));
        let deadline = Instant::now() + OVERALL_DEADLINE;
        let outcome = gemini
            .attempt(
                "k",
                "m",
                body.clone(),
                false,
                Call { deadline, long: true, id: 1 },
            )
            .await;
        assert!(matches!(outcome, Outcome::Done(_)));
        let received = server.join().unwrap();
        let end = received
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap();
        let head = String::from_utf8_lossy(&received[..end]).to_ascii_lowercase();
        assert_eq!(head.matches("content-length:").count(), 1);
        assert!(head.contains(&format!("content-length: {}", body.len())));
        assert!(!head.contains("transfer-encoding"));
        assert!(received[end + 4..] == body);

        let held = Arc::new(body);
        let mut rest = pieces(&held);
        assert!(rest.next().unwrap().unwrap() == held[..UPLOAD_CHUNK]);
        drop(held);
        assert_eq!(rest.next(), Some(Err("the attempt ended")));
    }

    #[test]
    fn mislabelled_images_pass_through_and_gifs_convert() {
        let encode = |format| {
            let mut out = Vec::new();
            image::DynamicImage::new_rgba8(2, 2)
                .write_to(&mut std::io::Cursor::new(&mut out), format)
                .unwrap();
            out
        };
        let png = encode(image::ImageFormat::Png);
        assert_eq!(
            normalize_image("image/gif", png.clone()),
            Some(("image/png".to_owned(), png))
        );
        let gif = normalize_image("image/gif", encode(image::ImageFormat::Gif)).unwrap();
        assert_eq!(gif.0, "image/png");
        assert!(normalize_image("image/gif", b"junk".to_vec()).is_none());
    }

    #[test]
    fn images_are_sent_as_what_their_bytes_are() {
        let encode = |width, height, format| {
            let mut out = Vec::new();
            image::DynamicImage::new_rgb8(width, height)
                .write_to(&mut std::io::Cursor::new(&mut out), format)
                .unwrap();
            out
        };
        let gif = encode(2, 2, image::ImageFormat::Gif);
        assert_eq!(
            needs_conversion("image/png", &gif),
            Some("image/gif")
        );
        assert_eq!(
            normalize_image("image/png", gif)
                .unwrap()
                .0,
            "image/png"
        );
        let jpeg = encode(2, 2, image::ImageFormat::Jpeg);
        assert_eq!(needs_conversion("image/webp", &jpeg), None);
        assert_eq!(
            normalize_image("image/webp", jpeg)
                .unwrap()
                .0,
            "image/jpeg"
        );
        assert!(normalize_image("image/png", b"not an image".to_vec()).is_none());
        assert_eq!(needs_conversion("application/pdf", b"BM"), None);
        // A wide screen capture converts; a bigger area does not, whatever its sides.
        assert!(
            normalize_image(
                "image/bmp",
                encode(2560, 1440, image::ImageFormat::Bmp)
            )
            .is_some()
        );
        assert!(
            normalize_image(
                "image/bmp",
                encode(2100, 2100, image::ImageFormat::Bmp)
            )
            .is_none()
        );
    }

    #[test]
    fn oversized_gif_is_refused_before_decoding() {
        let mut gif = Vec::new();
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(
                &mut std::io::Cursor::new(&mut gif),
                image::ImageFormat::Gif,
            )
            .unwrap();
        let huge = 11_000u16.to_le_bytes();
        gif[6..8].copy_from_slice(&huge);
        gif[8..10].copy_from_slice(&huge);
        let descriptor = gif
            .iter()
            .position(|&b| b == 0x2C)
            .unwrap();
        gif[descriptor + 5..descriptor + 7].copy_from_slice(&huge);
        gif[descriptor + 7..descriptor + 9].copy_from_slice(&huge);
        assert!(matches!(
            decode_bounded(&gif, image::ImageFormat::Gif),
            Err(image::ImageError::Limits(_))
        ));
        assert!(normalize_image("image/gif", gif).is_none());
    }
}
