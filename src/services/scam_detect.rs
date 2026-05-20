use std::{sync::Arc, time::Instant};

use anyhow::{Context as AnyhowContext, anyhow};
use async_trait::async_trait;
use futures::StreamExt as _;
use reqwest::{Client as ReqwestClient, multipart};
use serde::Deserialize;
use tokio::sync::{Semaphore, mpsc};
use twilight_model::{
    channel::{Attachment, Message},
    id::Id,
};

use crate::{
    configs::scam_detect::{SCAM_DETECT_CONFIG, ScamDetectConfig},
    context::Context,
    services::{broadcast::BroadcastService, spam},
};

#[derive(Clone)]
pub struct ScamDetectQueue {
    tx: Option<mpsc::Sender<ScamScanJob>>,
    config: Arc<ScamDetectConfig>,
}

#[derive(Clone)]
struct ScamScanJob {
    ctx: Arc<Context>,
    message: Message,
    quarantine_channel_id: u64,
    enqueued_at: Instant,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScanResponse {
    pub is_spam: bool,
    pub risk: f32,
    pub action: String,
    #[allow(dead_code)]
    pub score_raw: i32,
    pub reasons: Vec<String>,
    #[allow(dead_code)]
    pub ocr_text: String,
    #[allow(dead_code)]
    pub ocr_text_length: usize,
    #[allow(dead_code)]
    pub processing_ms: u128,
    #[allow(dead_code)]
    pub image_size: ImageSize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

#[async_trait]
pub trait ScamDetector: Send + Sync {
    async fn scan(&self, attachment: &Attachment) -> anyhow::Result<ScanResponse>;
}

struct HttpScamDetector {
    client: ReqwestClient,
    config: Arc<ScamDetectConfig>,
    scan_url: String,
}

impl Default for ScamDetectQueue {
    fn default() -> Self {
        Self::disabled()
    }
}

impl ScamDetectQueue {
    pub fn from_env() -> Self {
        let config = Arc::new(SCAM_DETECT_CONFIG.clone());
        let Some(url) = config.url.clone() else {
            return Self::disabled_with_config(config);
        };

        let client = ReqwestClient::builder()
            .connect_timeout(config.download_timeout)
            .timeout(config.download_timeout + config.scan_timeout)
            .build()
            .expect("failed to build scam detect HTTP client");
        let detector = Arc::new(HttpScamDetector::new(client, config.clone(), url));
        Self::with_detector(config, detector)
    }

    pub fn disabled() -> Self {
        Self::disabled_with_config(Arc::new(ScamDetectConfig::from_env()))
    }

    fn disabled_with_config(config: Arc<ScamDetectConfig>) -> Self {
        Self { tx: None, config }
    }

    pub fn with_detector(config: Arc<ScamDetectConfig>, detector: Arc<dyn ScamDetector>) -> Self {
        if !config.enabled() {
            return Self::disabled_with_config(config);
        }

        let (tx, mut rx) = mpsc::channel(config.queue_capacity);
        let permits = Arc::new(Semaphore::new(config.workers));
        let worker_config = config.clone();

        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                let permit = match permits.clone().acquire_owned().await {
                    Ok(permit) => permit,
                    Err(_) => break,
                };
                let detector = detector.clone();
                let config = worker_config.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    process_job(config, detector, job).await;
                });
            }
        });

        Self { tx: Some(tx), config }
    }

    pub fn enabled(&self) -> bool {
        self.tx.is_some()
    }

    pub fn try_enqueue(&self, ctx: Arc<Context>, message: &Message, quarantine_channel_id: u64) {
        let Some(tx) = &self.tx else {
            return;
        };

        if eligible_attachments(message, &self.config).is_empty() {
            return;
        }

        let guild_id = message.guild_id.map(|id| id.get());
        let channel_id = message.channel_id.get();
        let message_id = message.id.get();
        let user_id = message.author.id.get();
        let mut message = message.clone();
        message.attachments = eligible_attachments(&message, &self.config);
        let job = ScamScanJob { ctx, message, quarantine_channel_id, enqueued_at: Instant::now() };

        match tx.try_send(job) {
            Ok(()) => {
                metrics::counter!("scam_detect_jobs_total", "result" => "enqueued").increment(1);
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                metrics::counter!("scam_detect_jobs_total", "result" => "queue_full").increment(1);
                tracing::warn!(
                    guild_id,
                    channel_id,
                    message_id,
                    user_id,
                    "scam detect queue full; dropping image scan"
                );
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                metrics::counter!("scam_detect_jobs_total", "result" => "queue_closed")
                    .increment(1);
                tracing::warn!("scam detect queue closed; dropping image scan");
            }
        }
    }
}

async fn process_job(
    config: Arc<ScamDetectConfig>,
    detector: Arc<dyn ScamDetector>,
    mut job: ScamScanJob,
) {
    job.message.attachments = eligible_attachments(&job.message, &config);
    let Some(guild_id) = job.message.guild_id else {
        return;
    };

    if job.enqueued_at.elapsed() > config.job_ttl {
        metrics::counter!("scam_detect_jobs_total", "result" => "expired").increment(1);
        tracing::warn!(
            guild_id = guild_id.get(),
            channel_id = job.message.channel_id.get(),
            message_id = job.message.id.get(),
            user_id = job.message.author.id.get(),
            "scam detect job expired before processing"
        );
        return;
    }

    if spam::SpamService::is_quarantined(
        &job.ctx,
        guild_id.get(),
        job.message.author.id.get(),
    )
    .await
    {
        delete_detected_message(&job.ctx, &job.message).await;
        return;
    }

    for attachment in &job.message.attachments {
        let started_at = Instant::now();
        let result = tokio::time::timeout(config.scan_timeout, detector.scan(attachment))
            .await
            .map_err(|_| anyhow!("scan timed out"))
            .and_then(|result| result);
        metrics::histogram!("scam_detect_scan_seconds").record(started_at.elapsed().as_secs_f64());

        match result {
            Ok(scan) if scan_blocks(&scan) => {
                metrics::counter!("scam_detect_scans_total", "result" => "block").increment(1);
                quarantine_detected(&job.ctx, &job, &scan).await;
                return;
            }
            Ok(scan) => {
                let action = scan.action.clone();
                metrics::counter!("scam_detect_scans_total", "result" => "allow").increment(1);
                tracing::debug!(
                    guild_id = guild_id.get(),
                    channel_id = job.message.channel_id.get(),
                    message_id = job.message.id.get(),
                    user_id = job.message.author.id.get(),
                    attachment_id = attachment.id.get(),
                    risk = scan.risk,
                    action,
                    reasons = ?scan.reasons,
                    "scam detect scan did not block"
                );
            }
            Err(e) => {
                metrics::counter!("scam_detect_scans_total", "result" => "error").increment(1);
                tracing::warn!(
                    guild_id = guild_id.get(),
                    channel_id = job.message.channel_id.get(),
                    message_id = job.message.id.get(),
                    user_id = job.message.author.id.get(),
                    attachment_id = attachment.id.get(),
                    error = %e,
                    "scam detect scan failed"
                );
            }
        }
    }
}

impl HttpScamDetector {
    fn new(client: ReqwestClient, config: Arc<ScamDetectConfig>, base_url: String) -> Self {
        let scan_url = format!("{}/scan", base_url.trim_end_matches('/'));
        Self { client, config, scan_url }
    }

    async fn download_image(&self, attachment: &Attachment) -> anyhow::Result<Vec<u8>> {
        if attachment.size > self.config.max_upload_bytes() {
            return Err(anyhow!(
                "attachment size {} exceeds configured {} byte limit",
                attachment.size,
                self.config.max_upload_bytes()
            ));
        }

        let response = tokio::time::timeout(
            self.config.download_timeout,
            self.client.get(&attachment.url).send(),
        )
        .await
        .map_err(|_| anyhow!("image download timed out"))??;
        let mut stream = response
            .error_for_status()?
            .bytes_stream();
        let mut bytes = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let next_len = bytes.len().saturating_add(chunk.len());
            if next_len as u64 > self.config.max_upload_bytes() {
                return Err(anyhow!(
                    "downloaded image exceeds configured {} byte limit",
                    self.config.max_upload_bytes()
                ));
            }
            bytes.extend_from_slice(&chunk);
        }

        Ok(bytes)
    }
}

#[async_trait]
impl ScamDetector for HttpScamDetector {
    async fn scan(&self, attachment: &Attachment) -> anyhow::Result<ScanResponse> {
        let bytes = self.download_image(attachment).await?;
        let content_type = attachment
            .content_type
            .as_deref()
            .unwrap_or("image/png");
        let part = multipart::Part::bytes(bytes)
            .file_name(attachment.filename.clone())
            .mime_str(content_type)
            .context("invalid attachment content type")?;
        let form = multipart::Form::new().part("file", part);
        let mut request = self
            .client
            .post(&self.scan_url)
            .multipart(form);

        if let Some(token) = &self.config.token {
            request = request.header("X-Scan-Token", token);
        }

        let response = request
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }
}

fn eligible_attachments(message: &Message, config: &ScamDetectConfig) -> Vec<Attachment> {
    message
        .attachments
        .iter()
        .filter(|attachment| is_eligible_image(attachment, config))
        .take(config.max_images_per_message)
        .cloned()
        .collect()
}

fn is_eligible_image(attachment: &Attachment, config: &ScamDetectConfig) -> bool {
    if attachment.ephemeral || attachment.url.is_empty() || attachment.size == 0 {
        return false;
    }
    if attachment.size > config.max_upload_bytes() {
        return false;
    }
    if let Some(content_type) = attachment.content_type.as_deref() {
        return content_type.starts_with("image/");
    }
    attachment.width.is_some() || attachment.height.is_some()
}

fn scan_blocks(scan: &ScanResponse) -> bool {
    scan.is_spam
        || scan
            .action
            .eq_ignore_ascii_case("block")
}

async fn quarantine_detected(ctx: &Arc<Context>, job: &ScamScanJob, scan: &ScanResponse) {
    let Some(guild_id) = job.message.guild_id else {
        return;
    };
    let user_id = job.message.author.id;
    delete_detected_message(ctx, &job.message).await;

    let token = format!("{:06}", fastrand::u32(0..1_000_000));
    let token =
        match spam::quarantine::claim_token(ctx, guild_id.get(), user_id.get(), &token).await {
            Ok(token) => token,
            Err(_) => return,
        };

    if let Some(guild_ref) = ctx.cache.guild(guild_id)
        && let Ok(embeds) = spam::embed::quarantine_embed(
            &guild_ref,
            &job.message,
            job.quarantine_channel_id,
            &token,
        )
    {
        let channel_id = Id::new(job.quarantine_channel_id);
        if let Err(e) = ctx
            .http
            .create_message(channel_id)
            .content(&format!("<@{}>", user_id))
            .embeds(&embeds)
            .await
        {
            tracing::warn!(
                channel_id = channel_id.get(),
                user_id = user_id.get(),
                error = %e,
                "failed to send scam image quarantine notice"
            );
        }
    }

    tracing::warn!(
        guild_id = guild_id.get(),
        channel_id = job.message.channel_id.get(),
        message_id = job.message.id.get(),
        user_id = user_id.get(),
        risk = scan.risk,
        action = scan.action,
        reasons = ?scan.reasons,
        "scam image detected; quarantining member"
    );
    spam::quarantine::quarantine_member(ctx, guild_id, user_id, &token).await;
}

async fn delete_detected_message(ctx: &Arc<Context>, message: &Message) {
    let original = [(message.channel_id.get(), message.id.get())];
    BroadcastService::delete_replicas(ctx, &original).await;
    if let Err(e) = ctx
        .http
        .delete_message(message.channel_id, message.id)
        .await
    {
        tracing::warn!(
            channel_id = message.channel_id.get(),
            message_id = message.id.get(),
            error = %e,
            "failed to delete scam image message"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ScamDetectConfig {
        ScamDetectConfig {
            url: Some("http://example.test".to_owned()),
            token: None,
            queue_capacity: 1,
            workers: 1,
            max_images_per_message: 2,
            max_upload_mb: 1,
            download_timeout: std::time::Duration::from_secs(1),
            scan_timeout: std::time::Duration::from_secs(1),
            job_ttl: std::time::Duration::from_secs(1),
        }
    }

    fn attachment(content_type: Option<&str>, size: u64, url: &str) -> Attachment {
        Attachment {
            content_type: content_type.map(ToOwned::to_owned),
            ephemeral: false,
            duration_secs: None,
            filename: "image.png".to_owned(),
            flags: None,
            description: None,
            height: Some(100),
            id: Id::new(1),
            proxy_url: String::new(),
            size,
            title: None,
            url: url.to_owned(),
            waveform: None,
            width: Some(100),
        }
    }

    #[test]
    fn scan_blocks_only_block_results() {
        let block = ScanResponse {
            is_spam: false,
            risk: 0.8,
            action: "block".to_owned(),
            score_raw: 1,
            reasons: Vec::new(),
            ocr_text: String::new(),
            ocr_text_length: 0,
            processing_ms: 0,
            image_size: ImageSize { width: 1, height: 1 },
        };
        let review = ScanResponse { action: "review".to_owned(), risk: 0.7, ..block.clone() };

        assert!(scan_blocks(&block));
        assert!(!scan_blocks(&review));
    }

    #[test]
    fn filters_ineligible_images() {
        let cfg = config();
        assert!(is_eligible_image(
            &attachment(
                Some("image/png"),
                1024,
                "https://cdn.example/image.png"
            ),
            &cfg
        ));
        assert!(!is_eligible_image(
            &attachment(
                Some("text/plain"),
                1024,
                "https://cdn.example/file.txt"
            ),
            &cfg
        ));
        assert!(!is_eligible_image(
            &attachment(
                Some("image/png"),
                cfg.max_upload_bytes() + 1,
                "https://cdn.example/big.png"
            ),
            &cfg
        ));
        assert!(!is_eligible_image(
            &attachment(Some("image/png"), 1024, ""),
            &cfg
        ));
    }
}
