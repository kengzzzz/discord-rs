use std::sync::LazyLock;

use crate::utils::env::parse_env;

pub struct DiscordConfigs {
    pub discord_token: String,
}

pub static DISCORD_CONFIGS: LazyLock<DiscordConfigs> = LazyLock::new(|| DiscordConfigs {
    discord_token: parse_env("DISCORD_TOKEN", ""),
});
