use std::{sync::LazyLock, time::Duration};

use crate::utils::env::parse_env;

const DEFAULT_SCAM_DETECT_URL: &str = "http://ocr-scam-detect:8000";
const DEFAULT_SCAM_DETECT_TOKEN: &str = "";
const DEFAULT_SCAM_DETECT_QUEUE_CAPACITY: &str = "128";
const DEFAULT_SCAM_DETECT_WORKERS: &str = "2";
const DEFAULT_SCAM_DETECT_MAX_IMAGES_PER_MESSAGE: &str = "3";
const DEFAULT_SCAM_DETECT_MAX_UPLOAD_MB: &str = "10";
const DEFAULT_SCAM_DETECT_DOWNLOAD_TIMEOUT_SECS: &str = "10";
const DEFAULT_SCAM_DETECT_SCAN_TIMEOUT_SECS: &str = "30";
const DEFAULT_SCAM_DETECT_JOB_TTL_SECS: &str = "120";

#[derive(Debug, Clone)]
pub struct ScamDetectConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    pub queue_capacity: usize,
    pub workers: usize,
    pub max_images_per_message: usize,
    pub max_upload_mb: u64,
    pub download_timeout: Duration,
    pub scan_timeout: Duration,
    pub job_ttl: Duration,
}

impl ScamDetectConfig {
    pub fn from_env() -> Self {
        Self {
            url: env_string("SCAM_DETECT_URL", DEFAULT_SCAM_DETECT_URL),
            token: env_string("SCAM_DETECT_TOKEN", DEFAULT_SCAM_DETECT_TOKEN),
            queue_capacity: parse_env::<usize>(
                "SCAM_DETECT_QUEUE_CAPACITY",
                DEFAULT_SCAM_DETECT_QUEUE_CAPACITY,
            )
            .max(1),
            workers: parse_env::<usize>("SCAM_DETECT_WORKERS", DEFAULT_SCAM_DETECT_WORKERS).max(1),
            max_images_per_message: parse_env::<usize>(
                "SCAM_DETECT_MAX_IMAGES_PER_MESSAGE",
                DEFAULT_SCAM_DETECT_MAX_IMAGES_PER_MESSAGE,
            )
            .max(1),
            max_upload_mb: parse_env::<u64>(
                "SCAM_DETECT_MAX_UPLOAD_MB",
                DEFAULT_SCAM_DETECT_MAX_UPLOAD_MB,
            ),
            download_timeout: Duration::from_secs(parse_env::<u64>(
                "SCAM_DETECT_DOWNLOAD_TIMEOUT_SECS",
                DEFAULT_SCAM_DETECT_DOWNLOAD_TIMEOUT_SECS,
            )),
            scan_timeout: Duration::from_secs(parse_env::<u64>(
                "SCAM_DETECT_SCAN_TIMEOUT_SECS",
                DEFAULT_SCAM_DETECT_SCAN_TIMEOUT_SECS,
            )),
            job_ttl: Duration::from_secs(parse_env::<u64>(
                "SCAM_DETECT_JOB_TTL_SECS",
                DEFAULT_SCAM_DETECT_JOB_TTL_SECS,
            )),
        }
    }

    pub fn max_upload_bytes(&self) -> u64 {
        self.max_upload_mb
            .saturating_mul(1024)
            .saturating_mul(1024)
    }

    pub fn enabled(&self) -> bool {
        self.url.is_some()
    }
}

pub static SCAM_DETECT_CONFIG: LazyLock<ScamDetectConfig> =
    LazyLock::new(ScamDetectConfig::from_env);

fn env_string(name: &str, default: &str) -> Option<String> {
    let value = std::env::var(name).unwrap_or_else(|_| default.to_owned());
    let value = value.trim().to_owned();

    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENV_KEYS: &[&str] = &[
        "SCAM_DETECT_URL",
        "SCAM_DETECT_TOKEN",
        "SCAM_DETECT_QUEUE_CAPACITY",
        "SCAM_DETECT_WORKERS",
        "SCAM_DETECT_MAX_IMAGES_PER_MESSAGE",
        "SCAM_DETECT_MAX_UPLOAD_MB",
        "SCAM_DETECT_DOWNLOAD_TIMEOUT_SECS",
        "SCAM_DETECT_SCAN_TIMEOUT_SECS",
        "SCAM_DETECT_JOB_TTL_SECS",
    ];

    fn clear_env() {
        for key in ENV_KEYS {
            unsafe {
                std::env::remove_var(key);
            }
        }
    }

    #[test]
    fn from_env_uses_scam_detect_defaults() {
        clear_env();

        let config = ScamDetectConfig::from_env();

        assert_eq!(
            config.url.as_deref(),
            Some("http://ocr-scam-detect:8000")
        );
        assert_eq!(config.token, None);
        assert_eq!(config.queue_capacity, 128);
        assert_eq!(config.workers, 2);
        assert_eq!(config.max_images_per_message, 3);
        assert_eq!(config.max_upload_mb, 10);
        assert_eq!(config.download_timeout, Duration::from_secs(10));
        assert_eq!(config.scan_timeout, Duration::from_secs(30));
        assert_eq!(config.job_ttl, Duration::from_secs(120));
        assert!(config.enabled());
    }

    #[test]
    fn from_env_allows_url_and_token_overrides() {
        clear_env();
        unsafe {
            std::env::set_var("SCAM_DETECT_URL", " http://detector.local:9000 ");
            std::env::set_var("SCAM_DETECT_TOKEN", " secret ");
        }

        let config = ScamDetectConfig::from_env();

        assert_eq!(
            config.url.as_deref(),
            Some("http://detector.local:9000")
        );
        assert_eq!(config.token.as_deref(), Some("secret"));

        clear_env();
    }
}
