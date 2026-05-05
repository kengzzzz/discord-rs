use anyhow::Error as AnyError;
use chrono::Utc;
use deadpool_redis::Pool;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use twilight_http::{Error as HttpError, api_error::ApiError, error::ErrorType};
use twilight_model::{
    channel::{Attachment, Message},
    id::Id,
};

use crate::{
    context::Context,
    dbs::redis::{redis_delete, redis_get, redis_set_ex},
    services::{broadcast::BroadcastService, spam::quarantine},
};
use std::sync::Arc;

const SPAM_LIMIT: usize = 4;
const LOG_TTL: usize = 600;
const CAMPAIGN_LIMIT: usize = 4;
const CAMPAIGN_TTL: usize = 600;

#[derive(Serialize, Deserialize)]
struct SpamRecord {
    hash: String,
    histories: Vec<(u64, u64)>,
    timestamp: i64,
}

#[derive(Serialize, Deserialize)]
struct CampaignRecord {
    histories: Vec<(u64, u64)>,
    first_seen: i64,
    last_seen: i64,
}

pub enum LogOutcome {
    None,
    NewlyQuarantined(String),
    AlreadyQuarantined,
}

pub async fn clear_log(pool: &Pool, guild_id: u64, user_id: u64) {
    let key = exact_log_key(guild_id, user_id);
    redis_delete(pool, &key).await;

    let campaign_index_key = campaign_index_key(guild_id, user_id);
    if let Some(campaign_keys) = redis_get::<Vec<String>>(pool, &campaign_index_key).await {
        for key in campaign_keys {
            redis_delete(pool, &key).await;
        }
    }
    redis_delete(pool, &campaign_index_key).await;
}

pub async fn log_message(ctx: &Arc<Context>, guild_id: u64, message: &Message) -> LogOutcome {
    let exact_hash = hash_message(message);
    let campaign_hash = campaign_hash_message(message);
    let key = exact_log_key(guild_id, message.author.id.get());
    let now = Utc::now().timestamp();
    let mut record = redis_get(&ctx.redis, &key)
        .await
        .unwrap_or(SpamRecord {
            hash: exact_hash.clone(),
            histories: Vec::with_capacity(SPAM_LIMIT),
            timestamp: now,
        });

    if record.hash == exact_hash
        && !record
            .histories
            .iter()
            .any(|h| h.0 == message.channel_id.get())
    {
        record
            .histories
            .push((message.channel_id.get(), message.id.get()));
    } else if record.hash != exact_hash {
        record.hash = exact_hash;
        record.histories.clear();
        record
            .histories
            .push((message.channel_id.get(), message.id.get()));
    }
    record.timestamp = now;

    if record.histories.len() >= SPAM_LIMIT {
        return quarantine_detected(
            ctx,
            guild_id,
            message.author.id.get(),
            record.histories.clone(),
        )
        .await;
    }

    let mut campaign_record = load_campaign_record(
        &ctx.redis,
        guild_id,
        message.author.id.get(),
        &campaign_hash,
        now,
    )
    .await;
    upsert_campaign_history(
        &mut campaign_record.histories,
        message.channel_id.get(),
        message.id.get(),
    );
    campaign_record.last_seen = now;

    if campaign_record.histories.len() >= CAMPAIGN_LIMIT {
        return quarantine_detected(
            ctx,
            guild_id,
            message.author.id.get(),
            campaign_record.histories.clone(),
        )
        .await;
    }

    redis_set_ex(&ctx.redis, &key, &record, LOG_TTL).await;
    persist_campaign_record(
        &ctx.redis,
        guild_id,
        message.author.id.get(),
        &campaign_hash,
        &campaign_record,
    )
    .await;

    LogOutcome::None
}

async fn quarantine_detected(
    ctx: &Arc<Context>,
    guild_id: u64,
    user_id: u64,
    to_delete: Vec<(u64, u64)>,
) -> LogOutcome {
    clear_log(&ctx.redis, guild_id, user_id).await;
    BroadcastService::delete_replicas(ctx, &to_delete).await;
    let delete_ctx = ctx.clone();
    tokio::spawn(async move {
        for (c_id, m_id) in to_delete {
            if let Err(e) = delete_ctx
                .http
                .delete_message(Id::new(c_id), Id::new(m_id))
                .await
            {
                if is_unknown_message_error(&e) {
                    tracing::debug!(
                        channel_id = c_id,
                        message_id = m_id,
                        "spam message was already deleted"
                    );
                } else {
                    tracing::warn!(channel_id = c_id, message_id = m_id, error = %e, "failed to delete spam message");
                }
            }
        }
    });
    let token = format!("{:06}", fastrand::u32(0..1_000_000));
    match quarantine::claim_token(ctx, guild_id, user_id, &token).await {
        Ok(token) => LogOutcome::NewlyQuarantined(token),
        Err(Some(_)) => LogOutcome::AlreadyQuarantined,
        Err(None) => LogOutcome::AlreadyQuarantined,
    }
}

fn hash_message(message: &Message) -> String {
    let mut hasher = Sha256::new();
    hasher.update(message.content.as_bytes());
    hasher.update((message.attachments.len() as u64).to_be_bytes());

    let mut fingerprints: Vec<_> = message
        .attachments
        .iter()
        .map(attachment_fingerprint)
        .collect();
    fingerprints.sort_unstable();

    for fingerprint in fingerprints {
        hasher.update(fingerprint);
    }
    hex::encode(hasher.finalize())
}

fn campaign_hash_message(message: &Message) -> String {
    let mut hasher = Sha256::new();
    let normalized = normalize_campaign_content(&message.content);
    hasher.update(normalized.as_bytes());
    hasher.update([u8::from(!message.content.trim().is_empty())]);
    hasher.update([u8::from(content_has_link(&message.content))]);
    hasher.update(campaign_attachment_shape(message).as_bytes());
    hex::encode(hasher.finalize())
}

fn attachment_fingerprint(attachment: &Attachment) -> [u8; 41] {
    let mut fingerprint = [0_u8; 41];
    fingerprint[..8].copy_from_slice(&attachment.size.to_be_bytes());
    fingerprint[8..16].copy_from_slice(
        &attachment
            .width
            .unwrap_or_default()
            .to_be_bytes(),
    );
    fingerprint[16..24].copy_from_slice(
        &attachment
            .height
            .unwrap_or_default()
            .to_be_bytes(),
    );
    fingerprint[24] = u8::from(attachment.ephemeral);
    fingerprint[25..33].copy_from_slice(
        &attachment
            .duration_secs
            .unwrap_or_default()
            .to_bits()
            .to_be_bytes(),
    );

    let mut metadata = Sha256::new();
    metadata.update(
        attachment
            .content_type
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    metadata.update(
        attachment
            .description
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    metadata.update(
        attachment
            .title
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    metadata.update(
        attachment
            .waveform
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    let metadata = metadata.finalize();
    fingerprint[33..].copy_from_slice(&metadata[..8]);

    fingerprint
}

fn campaign_attachment_shape(message: &Message) -> String {
    let mut image_count = 0_u64;
    let mut video_count = 0_u64;
    let mut audio_count = 0_u64;
    let mut other_count = 0_u64;

    for attachment in &message.attachments {
        match attachment_bucket(attachment) {
            "image" => image_count += 1,
            "video" => video_count += 1,
            "audio" => audio_count += 1,
            _ => other_count += 1,
        }
    }

    format!(
        "count:{}|image:{}|video:{}|audio:{}|other:{}",
        message.attachments.len(),
        image_count,
        video_count,
        audio_count,
        other_count
    )
}

fn attachment_bucket(attachment: &Attachment) -> &'static str {
    if let Some(content_type) = attachment.content_type.as_deref() {
        if content_type.starts_with("image/") {
            return "image";
        }
        if content_type.starts_with("video/") {
            return "video";
        }
        if content_type.starts_with("audio/") {
            return "audio";
        }
    }

    if attachment.width.is_some() || attachment.height.is_some() {
        return "image";
    }

    if attachment.duration_secs.is_some() || attachment.waveform.is_some() {
        return "audio";
    }

    "other"
}

fn normalize_campaign_content(content: &str) -> String {
    content
        .split_whitespace()
        .map(normalize_campaign_token)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_campaign_token(raw: &str) -> String {
    let trimmed = raw.trim_matches(|c: char| {
        !c.is_ascii_alphanumeric() && !matches!(c, '<' | '>' | '@' | '#' | ':' | '/' | '.')
    });
    if trimmed.is_empty() {
        return String::new();
    }

    let lower = trimmed.to_ascii_lowercase();

    if is_discord_mention(&lower) {
        return "@mention".to_owned();
    }

    if let Some(host) = extract_link_host(&lower) {
        return format!("url:{host}");
    }

    replace_long_digit_runs(&lower)
}

fn is_discord_mention(token: &str) -> bool {
    (token.starts_with("<@") && token.ends_with('>'))
        || (token.starts_with("<#") && token.ends_with('>'))
        || (token.starts_with("<@&") && token.ends_with('>'))
}

fn extract_link_host(token: &str) -> Option<String> {
    if let Ok(url) = reqwest::Url::parse(token) {
        return url.host_str().map(str::to_owned);
    }

    if let Some(rest) = token.strip_prefix("www.") {
        return rest
            .split('/')
            .next()
            .map(|host| host.trim_end_matches('.').to_owned())
            .filter(|host| host.contains('.'));
    }

    let candidate = token
        .split('/')
        .next()
        .unwrap_or_default()
        .trim_end_matches('.');
    if candidate.contains('.') && !candidate.contains('@') {
        return Some(candidate.to_owned());
    }

    None
}

fn replace_long_digit_runs(token: &str) -> String {
    let mut normalized = String::with_capacity(token.len());
    let chars: Vec<char> = token.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        if chars[index].is_ascii_digit() {
            let start = index;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }

            if index - start >= 3 {
                normalized.push_str("{num}");
            } else {
                for character in &chars[start..index] {
                    normalized.push(*character);
                }
            }
        } else {
            normalized.push(chars[index]);
            index += 1;
        }
    }

    normalized
}

async fn load_campaign_record(
    pool: &Pool,
    guild_id: u64,
    user_id: u64,
    campaign_hash: &str,
    now: i64,
) -> CampaignRecord {
    redis_get(
        pool,
        &campaign_log_key(guild_id, user_id, campaign_hash),
    )
    .await
    .unwrap_or(CampaignRecord {
        histories: Vec::with_capacity(CAMPAIGN_LIMIT),
        first_seen: now,
        last_seen: now,
    })
}

async fn persist_campaign_record(
    pool: &Pool,
    guild_id: u64,
    user_id: u64,
    campaign_hash: &str,
    record: &CampaignRecord,
) {
    let key = campaign_log_key(guild_id, user_id, campaign_hash);
    redis_set_ex(pool, &key, record, CAMPAIGN_TTL).await;

    let index_key = campaign_index_key(guild_id, user_id);
    let mut index = redis_get::<Vec<String>>(pool, &index_key)
        .await
        .unwrap_or_default();
    if !index.iter().any(|entry| entry == &key) {
        index.push(key);
    }
    redis_set_ex(pool, &index_key, &index, CAMPAIGN_TTL).await;
}

fn upsert_campaign_history(histories: &mut Vec<(u64, u64)>, channel_id: u64, message_id: u64) {
    if let Some(entry) = histories
        .iter_mut()
        .find(|entry| entry.0 == channel_id)
    {
        entry.1 = message_id;
    } else {
        histories.push((channel_id, message_id));
    }
}

fn exact_log_key(guild_id: u64, user_id: u64) -> String {
    format!("spam:log:{guild_id}:{user_id}")
}

fn campaign_index_key(guild_id: u64, user_id: u64) -> String {
    format!("spam:campaign:{guild_id}:{user_id}")
}

fn campaign_log_key(guild_id: u64, user_id: u64, campaign_hash: &str) -> String {
    format!("spam:campaign:{guild_id}:{user_id}:{campaign_hash}")
}

fn content_has_link(content: &str) -> bool {
    content
        .split_whitespace()
        .any(|token| extract_link_host(&token.to_ascii_lowercase()).is_some())
}

fn is_unknown_message_error(error: &AnyError) -> bool {
    matches!(
        error.downcast_ref::<HttpError>().map(HttpError::kind),
        Some(ErrorType::Response {
            error: ApiError::General(api_error),
            ..
        }) if api_error.code == 10008
    )
}

#[cfg(test)]
#[path = "tests/log.rs"]
mod tests;
