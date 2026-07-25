use std::{sync::LazyLock, time::Duration};

use anyhow::bail;

use crate::utils::env::parse_env;

const DEFAULT_SCAM_DETECT_ENABLED: &str = "true";
const DEFAULT_SCAM_DETECT_QUEUE_CAPACITY: &str = "128";
const DEFAULT_SCAM_DETECT_WORKERS: &str = "2";
const DEFAULT_SCAM_DETECT_MAX_IMAGES_PER_MESSAGE: &str = "3";
const DEFAULT_SCAM_DETECT_MAX_UPLOAD_MB: &str = "10";
const DEFAULT_SCAM_DETECT_DOWNLOAD_TIMEOUT_SECS: &str = "10";
const DEFAULT_SCAM_DETECT_SCAN_TIMEOUT_SECS: &str = "30";
const DEFAULT_SCAM_DETECT_JOB_TTL_SECS: &str = "120";
const DEFAULT_SCAM_DETECT_MAX_IMAGE_WIDTH: &str = "1600";
const DEFAULT_SCAM_DETECT_MAX_DECODED_PIXELS: &str = "16777216";
const DEFAULT_SCAM_DETECT_OCR_TEXT_LIMIT: &str = "3000";
const DEFAULT_SCAM_DETECT_OCR_MIN_CHARS_FOR_PSM6: &str = "20";
const DEFAULT_SCAM_DETECT_OCR_MAX_CONCURRENT: &str = "1";
const DEFAULT_SCAM_DETECT_BLOCK_THRESHOLD: &str = "0.80";
const DEFAULT_SCAM_DETECT_REVIEW_THRESHOLD: &str = "0.55";
const DEFAULT_SCAM_DETECT_TESSERACT_LANG: &str = "eng";

#[derive(Debug, Clone)]
pub struct ScamDetectConfig {
    pub enabled: bool,
    pub queue_capacity: usize,
    pub workers: usize,
    pub max_images_per_message: usize,
    pub max_upload_mb: u64,
    pub download_timeout: Duration,
    pub scan_timeout: Duration,
    pub job_ttl: Duration,
    pub max_image_width: u32,
    pub max_decoded_pixels: u64,
    pub ocr_text_limit: usize,
    pub ocr_min_chars_for_psm6: usize,
    pub ocr_max_concurrent: usize,
    pub block_threshold: f32,
    pub review_threshold: f32,
    pub tesseract_lang: String,
    pub tessdata_dir: Option<String>,
}

impl ScamDetectConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: parse_env("SCAM_DETECT_ENABLED", DEFAULT_SCAM_DETECT_ENABLED),
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
            max_image_width: parse_env(
                "SCAM_DETECT_MAX_IMAGE_WIDTH",
                DEFAULT_SCAM_DETECT_MAX_IMAGE_WIDTH,
            ),
            max_decoded_pixels: parse_env(
                "SCAM_DETECT_MAX_DECODED_PIXELS",
                DEFAULT_SCAM_DETECT_MAX_DECODED_PIXELS,
            ),
            ocr_text_limit: parse_env(
                "SCAM_DETECT_OCR_TEXT_LIMIT",
                DEFAULT_SCAM_DETECT_OCR_TEXT_LIMIT,
            ),
            ocr_min_chars_for_psm6: parse_env(
                "SCAM_DETECT_OCR_MIN_CHARS_FOR_PSM6",
                DEFAULT_SCAM_DETECT_OCR_MIN_CHARS_FOR_PSM6,
            ),
            ocr_max_concurrent: parse_env::<usize>(
                "SCAM_DETECT_OCR_MAX_CONCURRENT",
                DEFAULT_SCAM_DETECT_OCR_MAX_CONCURRENT,
            )
            .max(1),
            block_threshold: parse_env(
                "SCAM_DETECT_BLOCK_THRESHOLD",
                DEFAULT_SCAM_DETECT_BLOCK_THRESHOLD,
            ),
            review_threshold: parse_env(
                "SCAM_DETECT_REVIEW_THRESHOLD",
                DEFAULT_SCAM_DETECT_REVIEW_THRESHOLD,
            ),
            tesseract_lang: env_string(
                "SCAM_DETECT_TESSERACT_LANG",
                DEFAULT_SCAM_DETECT_TESSERACT_LANG,
            ),
            tessdata_dir: env_optional_string("SCAM_DETECT_TESSDATA_DIR"),
        }
    }

    pub fn max_upload_bytes(&self) -> u64 {
        self.max_upload_mb
            .saturating_mul(1024)
            .saturating_mul(1024)
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.max_upload_mb == 0 {
            bail!("SCAM_DETECT_MAX_UPLOAD_MB must be greater than zero");
        }
        if self.max_decoded_pixels == 0 {
            bail!("SCAM_DETECT_MAX_DECODED_PIXELS must be greater than zero");
        }
        if self.ocr_text_limit == 0 {
            bail!("SCAM_DETECT_OCR_TEXT_LIMIT must be greater than zero");
        }
        if self.download_timeout.is_zero() || self.scan_timeout.is_zero() || self.job_ttl.is_zero()
        {
            bail!("scam detector timeouts and job TTL must be greater than zero");
        }
        if self.tesseract_lang.is_empty() {
            bail!("SCAM_DETECT_TESSERACT_LANG must not be empty");
        }
        if !valid_threshold(self.review_threshold) || !valid_threshold(self.block_threshold) {
            bail!("scam detector thresholds must be finite values between zero and one");
        }
        if self.review_threshold > self.block_threshold {
            bail!("SCAM_DETECT_REVIEW_THRESHOLD must not exceed SCAM_DETECT_BLOCK_THRESHOLD");
        }
        Ok(())
    }
}

pub static SCAM_DETECT_CONFIG: LazyLock<ScamDetectConfig> =
    LazyLock::new(ScamDetectConfig::from_env);

fn env_string(name: &str, default: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| default.to_owned())
        .trim()
        .to_owned()
}

fn env_optional_string(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn valid_threshold(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::env::test_support::EnvGuard;

    const ENV_KEYS: &[&str] = &[
        "SCAM_DETECT_ENABLED",
        "SCAM_DETECT_QUEUE_CAPACITY",
        "SCAM_DETECT_WORKERS",
        "SCAM_DETECT_MAX_IMAGES_PER_MESSAGE",
        "SCAM_DETECT_MAX_UPLOAD_MB",
        "SCAM_DETECT_DOWNLOAD_TIMEOUT_SECS",
        "SCAM_DETECT_SCAN_TIMEOUT_SECS",
        "SCAM_DETECT_JOB_TTL_SECS",
        "SCAM_DETECT_MAX_IMAGE_WIDTH",
        "SCAM_DETECT_MAX_DECODED_PIXELS",
        "SCAM_DETECT_OCR_TEXT_LIMIT",
        "SCAM_DETECT_OCR_MIN_CHARS_FOR_PSM6",
        "SCAM_DETECT_OCR_MAX_CONCURRENT",
        "SCAM_DETECT_BLOCK_THRESHOLD",
        "SCAM_DETECT_REVIEW_THRESHOLD",
        "SCAM_DETECT_TESSERACT_LANG",
        "SCAM_DETECT_TESSDATA_DIR",
    ];

    #[test]
    fn from_env_uses_scam_detect_defaults() {
        let _env = EnvGuard::acquire(ENV_KEYS);

        let config = ScamDetectConfig::from_env();

        assert!(config.enabled);
        assert_eq!(config.queue_capacity, 128);
        assert_eq!(config.workers, 2);
        assert_eq!(config.max_images_per_message, 3);
        assert_eq!(config.max_upload_mb, 10);
        assert_eq!(config.download_timeout, Duration::from_secs(10));
        assert_eq!(config.scan_timeout, Duration::from_secs(30));
        assert_eq!(config.job_ttl, Duration::from_secs(120));
        assert_eq!(config.max_image_width, 1600);
        assert_eq!(config.max_decoded_pixels, 16_777_216);
        assert_eq!(config.ocr_text_limit, 3000);
        assert_eq!(config.ocr_min_chars_for_psm6, 20);
        assert_eq!(config.ocr_max_concurrent, 1);
        assert_eq!(config.block_threshold, 0.80);
        assert_eq!(config.review_threshold, 0.55);
        assert_eq!(config.tesseract_lang, "eng");
        assert_eq!(config.tessdata_dir, None);
        assert!(config.enabled());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn from_env_allows_ocr_overrides() {
        let env = EnvGuard::acquire(ENV_KEYS);
        env.set("SCAM_DETECT_ENABLED", "false");
        env.set("SCAM_DETECT_TESSERACT_LANG", " tha ");
        env.set("SCAM_DETECT_TESSDATA_DIR", " /opt/tessdata ");
        env.set("SCAM_DETECT_MAX_DECODED_PIXELS", "42");

        let config = ScamDetectConfig::from_env();

        assert!(!config.enabled);
        assert_eq!(config.tesseract_lang, "tha");
        assert_eq!(
            config.tessdata_dir.as_deref(),
            Some("/opt/tessdata")
        );
        assert_eq!(config.max_decoded_pixels, 42);
    }

    #[test]
    fn validation_rejects_unsafe_ocr_settings() {
        let _env = EnvGuard::acquire(ENV_KEYS);
        let mut config = ScamDetectConfig::from_env();

        config.review_threshold = 0.9;
        config.block_threshold = 0.8;
        assert!(config.validate().is_err());

        config.review_threshold = 0.55;
        config.ocr_text_limit = 0;
        assert!(config.validate().is_err());
    }
}
