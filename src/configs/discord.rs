use std::sync::LazyLock;

use crate::utils::env::secret_or_default;

pub struct DiscordConfigs {
    pub discord_token: String,
}

pub static DISCORD_CONFIGS: LazyLock<DiscordConfigs> =
    LazyLock::new(|| DiscordConfigs { discord_token: secret_or_default("DISCORD_TOKEN", "") });
