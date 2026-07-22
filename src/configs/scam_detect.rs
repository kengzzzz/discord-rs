use std::{fmt, sync::LazyLock, time::Duration};

use crate::utils::env::{Secret, parse_env, secret};

const DEFAULT_SCAM_DETECT_URL: &str = "http://ocr-scam-detect:8000";
const DEFAULT_SCAM_DETECT_QUEUE_CAPACITY: &str = "128";
const DEFAULT_SCAM_DETECT_WORKERS: &str = "2";
const DEFAULT_SCAM_DETECT_MAX_IMAGES_PER_MESSAGE: &str = "3";
const DEFAULT_SCAM_DETECT_MAX_UPLOAD_MB: &str = "10";
const DEFAULT_SCAM_DETECT_DOWNLOAD_TIMEOUT_SECS: &str = "10";
const DEFAULT_SCAM_DETECT_SCAN_TIMEOUT_SECS: &str = "30";
const DEFAULT_SCAM_DETECT_JOB_TTL_SECS: &str = "120";

#[derive(Clone)]
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

/// Hand-written so the token cannot reach a log through a `{:?}` of this config
/// or of any struct holding one.
impl fmt::Debug for ScamDetectConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScamDetectConfig")
            .field("url", &self.url)
            .field(
                "token",
                &self
                    .token
                    .as_ref()
                    .map(|_| "<redacted>"),
            )
            .field("queue_capacity", &self.queue_capacity)
            .field("workers", &self.workers)
            .field(
                "max_images_per_message",
                &self.max_images_per_message,
            )
            .field("max_upload_mb", &self.max_upload_mb)
            .field("download_timeout", &self.download_timeout)
            .field("scan_timeout", &self.scan_timeout)
            .field("job_ttl", &self.job_ttl)
            .finish()
    }
}

impl ScamDetectConfig {
    pub fn from_env() -> Self {
        Self {
            url: env_string("SCAM_DETECT_URL", DEFAULT_SCAM_DETECT_URL),
            token: secret_string("SCAM_DETECT_TOKEN"),
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

/// A legacy env value keeps [`env_string`]'s `trim()`-then-empty-is-absent
/// behavior, so adding file support cannot change how an already-deployed
/// `SCAM_DETECT_TOKEN` is interpreted. File values are used as-is.
fn secret_string(name: &str) -> Option<String> {
    match secret(name) {
        Ok(Some(Secret::File(value))) => Some(value),
        Ok(Some(Secret::Env(value))) => {
            let value = value.trim().to_owned();
            (!value.is_empty()).then_some(value)
        }
        Ok(None) => None,
        Err(error) => panic!("{error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::env::test_support::EnvGuard;

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

    #[test]
    fn from_env_uses_scam_detect_defaults() {
        let _env = EnvGuard::acquire(ENV_KEYS);

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
        let env = EnvGuard::acquire(ENV_KEYS);
        env.set("SCAM_DETECT_URL", " http://detector.local:9000 ");
        env.set("SCAM_DETECT_TOKEN", " secret ");

        let config = ScamDetectConfig::from_env();

        assert_eq!(
            config.url.as_deref(),
            Some("http://detector.local:9000")
        );
        assert_eq!(config.token.as_deref(), Some("secret"));
    }
}
