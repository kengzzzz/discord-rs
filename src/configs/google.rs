use std::sync::LazyLock;

use crate::utils::env::{parse_env, secret_or_default};

pub struct GoogleConfigs {
    pub api_key: String,
    pub base_prompt: String,
}

pub static GOOGLE_CONFIGS: LazyLock<GoogleConfigs> = LazyLock::new(|| GoogleConfigs {
    api_key: secret_or_default("GOOGLE_API_KEY", ""),
    base_prompt: parse_env("AI_BASE_PROMPT", ""),
});
