use anyhow::Context as AnyhowContext;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use futures::{StreamExt, stream::FuturesUnordered};
use google_ai_rs::Part;
use reqwest::{Body, header::CONTENT_TYPE};
use std::str::FromStr;
use std::sync::Arc;
use twilight_model::channel::Attachment;

use crate::{configs::google::GOOGLE_CONFIGS, context::Context};

const CONCURRENCY: usize = 5;

async fn handle_attachment(
    ctx: Arc<Context>,
    a: Attachment,
) -> anyhow::Result<Option<(String, String)>> {
    if let (Some(ct), Ok(resp)) = (
        a.content_type.clone(),
        ctx.reqwest.get(a.url).send().await?.error_for_status(),
    ) {
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
        let resp = ctx
            .reqwest
            .post(upload_url)
            .headers(headers)
            .body(stream)
            .send()
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

async fn run(
    idx: usize,
    ctx: Arc<Context>,
    a: Attachment,
) -> (usize, anyhow::Result<Option<(String, String)>>) {
    (idx, handle_attachment(ctx, a).await)
}

pub async fn append_attachments(
    ctx: &Arc<Context>,
    parts: &mut Vec<Part>,
    attachments: Vec<Attachment>,
    owner: &str,
) -> anyhow::Result<Vec<String>> {
    let mut in_flight = FuturesUnordered::new();
    let len = attachments.len();
    let mut iter = attachments.into_iter().enumerate();
    let mut results: Vec<Option<(String, String)>> = vec![None; len];

    for _ in 0..CONCURRENCY {
        if let Some((idx, a)) = iter.next() {
            let ctx = Arc::clone(ctx);
            in_flight.push(run(idx, ctx, a));
        }
    }

    while let Some((idx, res)) = in_flight.next().await {
        if let Some((ct, uri)) = res? {
            results[idx] = Some((ct, uri));
        }

        if let Some((idx, a)) = iter.next() {
            let ctx = Arc::clone(ctx);
            in_flight.push(run(idx, ctx, a));
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
