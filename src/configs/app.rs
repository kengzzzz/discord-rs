use std::sync::LazyLock;

use crate::utils::env::parse_env;

pub struct AppConfig {
    pub env: String,
}

impl AppConfig {
    pub fn is_atlas(&self) -> bool {
        !self.env.eq_ignore_ascii_case("local")
    }
}

pub static APP_CONFIG: LazyLock<AppConfig> =
    LazyLock::new(|| AppConfig { env: parse_env("APP_ENV", "development") });
