use anyhow::{Context, anyhow};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    api_key: String,
}

pub enum Auth {
    ApiKey(String),
}

impl Client {
    pub async fn new(auth: Auth) -> anyhow::Result<Self> {
        let Auth::ApiKey(api_key) = auth;
        Ok(Self { http: reqwest::Client::new(), api_key })
    }

    pub fn generative_model(&self, model: &str) -> GenerativeModel<'_> {
        GenerativeModel { client: self, model: model.to_string(), system_instruction: None }
    }
}

pub struct GenerativeModel<'a> {
    client: &'a Client,
    model: String,
    system_instruction: Option<String>,
}

impl GenerativeModel<'_> {
    pub fn with_system_instruction(mut self, system: &str) -> Self {
        self.system_instruction = Some(system.to_string());
        self
    }

    pub async fn generate_content(self, contents: Vec<Content>) -> anyhow::Result<Response> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );
        let mut request = self
            .client
            .http
            .post(url)
            .query(&[("key", self.client.api_key.as_str())]);

        let body = GenerateContentRequest {
            system_instruction: self
                .system_instruction
                .map(|text| Content { role: "user".to_string(), parts: vec![Part::text(text)] }),
            contents,
        };
        request = request.json(&body);

        let response = request
            .send()
            .await
            .context("failed to call Gemini API")?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read Gemini API response")?;

        if !status.is_success() {
            return Err(api_error(status, &body));
        }

        serde_json::from_str(&body).context("failed to decode Gemini API response")
    }
}

fn api_error(status: StatusCode, body: &str) -> anyhow::Error {
    let message = serde_json::from_str::<ApiErrorEnvelope>(body)
        .ok()
        .and_then(|envelope| envelope.error)
        .map(|error| match error.status {
            Some(code) if !code.is_empty() => format!(
                "Gemini API error {status} ({code}): {}",
                error.message
            ),
            _ => format!("Gemini API error {status}: {}", error.message),
        })
        .unwrap_or_else(|| format!("Gemini API error {status}: {body}"));
    anyhow!(message)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Content {
    pub role: String,
    pub parts: Vec<Part>,
}

impl From<Part> for Content {
    fn from(part: Part) -> Self {
        Self { role: "user".to_string(), parts: vec![part] }
    }
}

impl From<Vec<Part>> for Content {
    fn from(parts: Vec<Part>) -> Self {
        Self { role: "user".to_string(), parts }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "fileData", skip_serializing_if = "Option::is_none")]
    pub file_data: Option<FileData>,
}

impl Part {
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: Some(text.into()), file_data: None }
    }

    pub fn file_data(mime_type: impl Into<String>, file_uri: impl Into<String>) -> Self {
        Self {
            text: None,
            file_data: Some(FileData { mime_type: mime_type.into(), file_uri: file_uri.into() }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "fileUri")]
    pub file_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Candidate {
    pub content: Option<Content>,
}

#[derive(Serialize)]
struct GenerateContentRequest {
    #[serde(
        rename = "systemInstruction",
        skip_serializing_if = "Option::is_none"
    )]
    system_instruction: Option<Content>,
    contents: Vec<Content>,
}

#[derive(Deserialize)]
struct ApiErrorEnvelope {
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    message: String,
    status: Option<String>,
}
