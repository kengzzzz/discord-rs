use std::sync::LazyLock;

use crate::utils::env::parse_env;

pub struct RedisConfigs {
    pub redis_url: String,
}

pub static REDIS_CONFIGS: LazyLock<RedisConfigs> =
    LazyLock::new(|| RedisConfigs { redis_url: parse_env("REDIS_URL", "redis://redis:6379") });
