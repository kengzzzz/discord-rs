use std::sync::{Arc, LazyLock};

use once_cell::sync::Lazy;
use twilight_cache_inmemory::{DefaultInMemoryCache, ResourceType};
use twilight_http::Client;

use crate::utils::env::parse_env;

pub struct DiscordConfigs {
    pub discord_token: String,
}

pub static DISCORD_CONFIGS: LazyLock<DiscordConfigs> = LazyLock::new(|| DiscordConfigs {
    discord_token: parse_env("DISCORD_TOKEN", ""),
});

pub static HTTP: Lazy<Arc<Client>> = Lazy::new(|| {
    let token = DISCORD_CONFIGS.discord_token.clone();
    Arc::new(Client::new(token))
});

pub static CACHE: Lazy<Arc<DefaultInMemoryCache>> = Lazy::new(|| {
    let cache = DefaultInMemoryCache::builder()
        .resource_types(
            ResourceType::GUILD
                | ResourceType::CHANNEL
                | ResourceType::MESSAGE
                | ResourceType::ROLE
                | ResourceType::MEMBER
                | ResourceType::USER_CURRENT,
        )
        .build();
    Arc::new(cache)
});
