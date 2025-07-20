use twilight_model::channel::message::EmojiReactionType;

use crate::{configs::Reaction, dbs::mongo::models::role::RoleEnum};

pub fn emoji_to_role_enum(emoji: &EmojiReactionType) -> Option<RoleEnum> {
    match emoji {
        EmojiReactionType::Unicode { name } => match name.as_str() {
            e if e == Reaction::Riven.emoji() => Some(RoleEnum::RivenSilver),
            e if e == Reaction::Helminth.emoji() => Some(RoleEnum::Helminth),
            e if e == Reaction::UmbraForma.emoji() => Some(RoleEnum::UmbralForma),
            e if e == Reaction::Eidolon.emoji() => Some(RoleEnum::Eidolon),
            e if e == Reaction::Live.emoji() => Some(RoleEnum::Live),
            _ => None,
        },
        _ => None,
    }
}

pub fn role_enum_to_emoji(role: &RoleEnum) -> Option<&'static str> {
    match role {
        RoleEnum::RivenSilver => Some(Reaction::Riven.emoji()),
        RoleEnum::Helminth => Some(Reaction::Helminth.emoji()),
        RoleEnum::UmbralForma => Some(Reaction::UmbraForma.emoji()),
        RoleEnum::Eidolon => Some(Reaction::Eidolon.emoji()),
        RoleEnum::Live => Some(Reaction::Live.emoji()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/reaction.rs"]
mod tests;
