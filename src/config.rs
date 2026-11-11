use std::{env, fs, net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context as _, bail};

#[derive(Debug, Clone)]
pub struct Config {
    pub discord_token: String,
    pub database_path: PathBuf,
    pub http_addr: SocketAddr,
    pub ai: AiConfig,
    pub scam: ScamConfig,
}

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub api_key: Option<String>,
    pub translate_models: Vec<String>,
    pub chat_models: Vec<String>,
    pub grounded_model: Option<String>,
    pub router_model: String,
    pub search_provider: Option<SearchProvider>,
    pub search_key: Option<String>,
    pub extra_prompt: String,
    pub history_turns: usize,
    pub history_ttl: Duration,
    pub cooldown: Duration,
    pub max_inline_bytes: usize,
    pub daily_limit: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchProvider {
    Exa,
    Tavily,
}

impl FromStr for SearchProvider {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "exa" => Ok(Self::Exa),
            "tavily" => Ok(Self::Tavily),
            other => Err(format!(
                "unknown search provider {other:?} (expected exa or tavily)"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScamConfig {
    pub enabled: bool,
    pub queue_capacity: usize,
    pub max_images_per_message: usize,
    pub max_upload_bytes: u64,
    pub download_timeout: Duration,
    pub scan_timeout: Duration,
    pub job_ttl: Duration,
    pub max_image_width: u32,
    pub max_decoded_pixels: u64,
    pub worker_memory_mb: u64,
    pub text_limit: usize,
    pub block_threshold: f32,
    pub review_threshold: f32,
    pub models_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let discord_token = secret("DISCORD_TOKEN")?.context("DISCORD_TOKEN is required")?;
        Ok(Self {
            discord_token,
            database_path: var_or(
                "DATABASE_PATH",
                PathBuf::from("/data/bot.sqlite3"),
            )?,
            http_addr: var_or(
                "HTTP_ADDR",
                SocketAddr::from(([0, 0, 0, 0], 8080)),
            )?,
            ai: AiConfig::from_env()?,
            scam: ScamConfig::from_env()?,
        })
    }
}

impl AiConfig {
    pub(crate) fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            api_key: secret("GOOGLE_API_KEY")?,
            translate_models: list_or(
                "AI_TRANSLATE_MODELS",
                &[
                    "gemma-4-26b-a4b-it",
                    "gemini-3.5-flash-lite",
                    "gemini-3.1-flash-lite",
                    "gemma-4-31b-it",
                ],
            ),
            chat_models: list_or(
                "AI_CHAT_MODELS",
                &[
                    "gemma-4-26b-a4b-it",
                    "gemini-3.5-flash-lite",
                    "gemini-3.8-flash",
                    "gemma-4-31b-it",
                ],
            ),
            router_model: var_or(
                "AI_ROUTER_MODEL",
                "gemini-3.5-flash-lite".to_owned(),
            )?,
            search_provider: optional_var("AI_SEARCH_PROVIDER")?,
            search_key: secret("AI_SEARCH_API_KEY")?,
            grounded_model: non_empty_env("AI_GROUNDED_MODEL"),
            extra_prompt: var_or("AI_BASE_PROMPT", String::new())?,
            history_turns: var_or("AI_HISTORY_TURNS", 12)?,
            history_ttl: match var_or("AI_HISTORY_TTL_SECS", 24 * 60 * 60)? {
                secs @ 1..=31_536_000 => Duration::from_secs(secs),
                _ => anyhow::bail!("AI_HISTORY_TTL_SECS must be 1..=31536000 (one year)"),
            },
            cooldown: Duration::from_secs(var_or("AI_COOLDOWN_SECS", 3)?),
            daily_limit: var_or("AI_DAILY_LIMIT", 100)?,
            // Each request holds about 2.3x this; Gemini refuses inline requests over 20 MB.
            max_inline_bytes: match var_or("AI_MAX_INLINE_MB", 15usize)? {
                mb @ 1..=15 => mb << 20,
                _ => bail!("AI_MAX_INLINE_MB must be 1..=15"),
            },
        })
    }
}

impl ScamConfig {
    pub(crate) fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            enabled: var_or("SCAM_DETECT_ENABLED", true)?,
            queue_capacity: var_or("SCAM_DETECT_QUEUE_CAPACITY", 128)?,
            max_images_per_message: var_or("SCAM_DETECT_MAX_IMAGES_PER_MESSAGE", 10)?,
            max_upload_bytes: match var_or("SCAM_DETECT_MAX_UPLOAD_MB", 10u64)? {
                mb @ 1..=10 => mb << 20,
                _ => bail!("SCAM_DETECT_MAX_UPLOAD_MB must be 1..=10"),
            },
            download_timeout: Duration::from_secs(var_or("SCAM_DETECT_DOWNLOAD_TIMEOUT_SECS", 10)?),
            scan_timeout: Duration::from_secs(var_or("SCAM_DETECT_SCAN_TIMEOUT_SECS", 30)?),
            job_ttl: Duration::from_secs(var_or("SCAM_DETECT_JOB_TTL_SECS", 120)?),
            max_image_width: var_or("SCAM_DETECT_MAX_IMAGE_WIDTH", 1600)?,
            max_decoded_pixels: var_or("SCAM_DETECT_MAX_DECODED_PIXELS", 16_777_216)?,
            // Above 272 the worst case can hit the 512 MiB container limit.
            worker_memory_mb: match var_or("SCAM_DETECT_WORKER_MEMORY_MB", 272u64)? {
                mb @ 128..=272 => mb,
                _ => bail!("SCAM_DETECT_WORKER_MEMORY_MB must be 128..=272"),
            },
            text_limit: var_or("SCAM_DETECT_OCR_TEXT_LIMIT", 3000)?,
            block_threshold: var_or("SCAM_DETECT_BLOCK_THRESHOLD", 0.80)?,
            review_threshold: var_or("SCAM_DETECT_REVIEW_THRESHOLD", 0.55)?,
            models_dir: var_or(
                "SCAM_DETECT_MODELS_DIR",
                PathBuf::from("/usr/share/ocrs"),
            )?,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        // A full queue relays unscanned, so a small one lets a few members' images through.
        if !(32..=128).contains(&self.queue_capacity) || self.max_images_per_message == 0 {
            bail!("scam detect queue capacity must be 32..=128 and image count positive");
        }
        // OCR memory limits were measured up to this size.
        if self.max_image_width < 32 || !(800 * 800..=16_777_216).contains(&self.max_decoded_pixels)
        {
            bail!("scam detect pixel limit must be 640000..=16777216 and width at least 32");
        }
        // Shorter lets the backstop end scans still within their step limits.
        if !(Duration::from_secs(20)..=Duration::from_secs(300)).contains(&self.scan_timeout)
            || !(Duration::from_secs(1)..=Duration::from_secs(300)).contains(&self.download_timeout)
        {
            bail!("scam detect scan timeout must be 20..=300 s and download timeout 1..=300 s");
        }
        if !(0.0..=1.0).contains(&self.review_threshold)
            || !(0.0..=1.0).contains(&self.block_threshold)
            || self.review_threshold > self.block_threshold
        {
            bail!("scam detect thresholds must satisfy 0 <= review <= block <= 1");
        }
        Ok(())
    }
}

pub fn secret(name: &str) -> anyhow::Result<Option<String>> {
    let file_var = format!("{name}_FILE");
    if let Some(path) = non_empty_env(&file_var) {
        let value = fs::read_to_string(&path)
            .with_context(|| format!("{file_var} points to {path}, which is unreadable"))?;
        let value = value.trim();
        if value.is_empty() {
            bail!("{file_var} points to {path}, which is empty");
        }
        return Ok(Some(value.to_owned()));
    }
    Ok(non_empty_env(name))
}

fn list_or(name: &str, default: &[&str]) -> Vec<String> {
    match non_empty_env(name) {
        Some(raw) => raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        None => default
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn optional_var<T>(name: &str) -> anyhow::Result<Option<T>>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    non_empty_env(name)
        .map(|raw| {
            raw.trim()
                .parse()
                .map_err(|e| anyhow::anyhow!("{name}={raw:?} is invalid: {e}"))
        })
        .transpose()
}

fn var_or<T>(name: &str, default: T) -> anyhow::Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    Ok(optional_var(name)?.unwrap_or(default))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scam_config_is_valid() {
        let config = ScamConfig {
            enabled: true,
            queue_capacity: 32,
            max_images_per_message: 1,
            max_upload_bytes: 1,
            download_timeout: Duration::from_secs(1),
            scan_timeout: Duration::from_secs(30),
            job_ttl: Duration::from_secs(1),
            max_image_width: 1600,
            max_decoded_pixels: 16_777_216,
            worker_memory_mb: 256,
            text_limit: 1,
            block_threshold: 0.8,
            review_threshold: 0.55,
            models_dir: PathBuf::from("/usr/share/ocrs"),
        };
        assert!(config.validate().is_ok());
        for bad in [
            ScamConfig { scan_timeout: Duration::ZERO, ..config.clone() },
            ScamConfig { download_timeout: Duration::ZERO, ..config.clone() },
            ScamConfig { max_decoded_pixels: 167_772_160, ..config.clone() },
            ScamConfig { max_decoded_pixels: 1, ..config.clone() },
            ScamConfig { queue_capacity: 31, ..config.clone() },
            ScamConfig { queue_capacity: 129, ..config.clone() },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        assert!(
            ScamConfig { review_threshold: 0.9, ..config }
                .validate()
                .is_err()
        );
    }
}
