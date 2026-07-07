use std::sync::LazyLock;

use crate::utils::env::parse_env;

pub struct AppConfig {
    pub env: String,
}

pub static APP_CONFIG: LazyLock<AppConfig> =
    LazyLock::new(|| AppConfig { env: parse_env("APP_ENV", "development") });
