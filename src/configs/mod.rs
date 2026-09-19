use std::sync::LazyLock;

pub mod app;
pub mod discord;
pub mod google;
pub mod mongo;
pub mod notifications;
pub mod redis;
pub mod scam_detect;

pub const CACHE_PREFIX: &str = "discord-bot";

/// Forces the secret-bearing `LazyLock`s before the bot connects.
///
/// `*_FILE` would abort the process mid-traffic instead of at startup.
pub fn init_secrets() {
    LazyLock::force(&discord::DISCORD_CONFIGS);
    LazyLock::force(&google::GOOGLE_CONFIGS);
    LazyLock::force(&mongo::MONGO_CONFIGS);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    Plus,
    Minus,
    Left,
    Right,
    Load,
    Success,
    Fail,
    Live,
    Riven,
    Helminth,
    UmbraForma,
    Eidolon,
    Warframe,
    Soulframe,
}

impl Reaction {
    pub fn emoji(self) -> &'static str {
        match self {
            Reaction::Plus => "➕",
            Reaction::Minus => "➖",
            Reaction::Left => "👈",
            Reaction::Right => "👉",
            Reaction::Load => "⌛",
            Reaction::Success => "✅",
            Reaction::Fail => "❌",
            Reaction::Live => "⏰",
            Reaction::Riven => "🍆",
            Reaction::Helminth => "🐙",
            Reaction::UmbraForma => "🧩",
            Reaction::Eidolon => "⚔️",
            Reaction::Warframe => "🪷",
            Reaction::Soulframe => "🐺",
        }
    }
}
