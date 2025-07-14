use anyhow::Context as AnyhowContext;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use google_ai_rs::Part;
use reqwest::{Body, header::CONTENT_TYPE};
use std::str::FromStr;
use std::sync::Arc;
use twilight_model::channel::Attachment;

use crate::{configs::google::GOOGLE_CONFIGS, context::Context, services::http::HttpService};

pub async fn append_attachments(
    ctx: &Arc<Context>,
    parts: &mut Vec<Part>,
    attachments: Vec<Attachment>,
    owner: &str,
) -> anyhow::Result<Vec<String>> {
    let mut urls = Vec::new();
    for a in attachments {
        if let (Some(ct), Ok(resp)) = (
            a.content_type.as_deref(),
            HttpService::get(ctx.reqwest.as_ref(), &a.url).await,
        ) {
            let label = format!("Attachment from {owner}:");
            parts.push(Part::text(&label));
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
            let resp = HttpService::post(ctx.reqwest.as_ref(), upload_url)
                .headers(headers)
                .body(stream)
                .send()
                .await?
                .error_for_status()?;
            let json: serde_json::Value = resp.json().await?;
            let uri = json["file"]["uri"].as_str().context("Missing file uri")?;
            parts.push(Part::file_data(ct, uri));
            urls.push(uri.to_string());
        }
    }
    Ok(urls)
}
