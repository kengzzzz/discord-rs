use std::sync::LazyLock;

use crate::utils::env::parse_env;

pub struct GoogleConfigs {
    pub api_key: String,
    pub base_prompt: String,
}

pub static GOOGLE_CONFIGS: LazyLock<GoogleConfigs> = LazyLock::new(|| GoogleConfigs {
    api_key: parse_env("GOOGLE_API_KEY", ""),
    base_prompt: parse_env("AI_BASE_PROMPT", ""),
});
