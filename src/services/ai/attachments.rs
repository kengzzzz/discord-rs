use anyhow::Context as AnyhowContext;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use futures::{StreamExt, stream::FuturesUnordered};
use google_ai_rs::Part;
use reqwest::{Body, header::CONTENT_TYPE};
use std::str::FromStr;
use twilight_model::channel::Attachment;

use crate::configs::google::GOOGLE_CONFIGS;

use async_trait::async_trait;

#[async_trait]
pub trait AttachmentHttp {
    async fn get(&self, url: &str) -> reqwest::Result<reqwest::Response>;

    async fn post(
        &self,
        url: reqwest::Url,
        headers: HeaderMap,
        body: Body,
    ) -> reqwest::Result<reqwest::Response>;
}

#[async_trait]
impl AttachmentHttp for reqwest::Client {
    async fn get(&self, url: &str) -> reqwest::Result<reqwest::Response> {
        self.get(url).send().await
    }

    async fn post(
        &self,
        url: reqwest::Url,
        headers: HeaderMap,
        body: Body,
    ) -> reqwest::Result<reqwest::Response> {
        self.post(url).headers(headers).body(body).send().await
    }
}

const CONCURRENCY: usize = 5;

async fn handle_attachment<H>(http: &H, a: Attachment) -> anyhow::Result<Option<(String, String)>>
where
    H: AttachmentHttp + Sync,
{
    if let Some(ct) = a.content_type.clone() {
        let resp = http.get(&a.url).await?.error_for_status()?;
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
        let resp = http
            .post(upload_url, headers, stream)
            .await?
            .error_for_status()?;
        let json: serde_json::Value = resp.json().await?;
        let uri = json["file"]["uri"]
            .as_str()
            .context("Missing file uri")?
            .to_string();
        Ok(Some((ct, uri)))
    } else {
        Ok(None)
    }
}

async fn run<H>(
    idx: usize,
    http: &H,
    a: Attachment,
) -> (usize, anyhow::Result<Option<(String, String)>>)
where
    H: AttachmentHttp + Sync,
{
    (idx, handle_attachment(http, a).await)
}

pub async fn append_attachments<H>(
    http: &H,
    parts: &mut Vec<Part>,
    attachments: Vec<Attachment>,
    owner: &str,
) -> anyhow::Result<Vec<String>>
where
    H: AttachmentHttp + Sync,
{
    let mut in_flight = FuturesUnordered::new();
    let len = attachments.len();
    let mut iter = attachments.into_iter().enumerate();
    let mut results: Vec<Option<(String, String)>> = vec![None; len];

    for _ in 0..CONCURRENCY {
        if let Some((idx, a)) = iter.next() {
            in_flight.push(run(idx, http, a));
        }
    }

    while let Some((idx, res)) = in_flight.next().await {
        if let Some((ct, uri)) = res? {
            results[idx] = Some((ct, uri));
        }

        if let Some((idx, a)) = iter.next() {
            in_flight.push(run(idx, http, a));
        }
    }

    let mut urls = Vec::with_capacity(results.len());
    for res in results.into_iter().flatten() {
        let (ct, uri) = res;
        let label = format!("Attachment from {owner}:");
        parts.push(Part::text(&label));
        parts.push(Part::file_data(&ct, &uri));
        urls.push(uri);
    }

    Ok(urls)
}
