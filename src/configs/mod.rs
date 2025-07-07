pub mod app;
pub mod discord;
pub mod google;
pub mod mongo;
pub mod notifications;
pub mod redis;

pub const CACHE_PREFIX: &str = "discord-bot";

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
        }
    }
}
