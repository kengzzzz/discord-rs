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
mod tests {
    use super::*;
    use twilight_model::id::{Id, marker::EmojiMarker};

    #[test]
    fn test_emoji_to_role_enum_known() {
        let emoji = EmojiReactionType::Unicode {
            name: Reaction::Riven.emoji().to_string(),
        };

        assert_eq!(emoji_to_role_enum(&emoji), Some(RoleEnum::RivenSilver));
    }

    #[test]
    fn test_emoji_to_role_enum_unknown_unicode() {
        let emoji = EmojiReactionType::Unicode {
            name: "❓".to_string(),
        };

        assert_eq!(emoji_to_role_enum(&emoji), None);
    }

    #[test]
    fn test_emoji_to_role_enum_custom() {
        let emoji = EmojiReactionType::Custom {
            animated: false,
            id: Id::<EmojiMarker>::new(1),
            name: Some("foo".to_string()),
        };

        assert_eq!(emoji_to_role_enum(&emoji), None);
    }

    #[test]
    fn test_role_enum_to_emoji_known() {
        assert_eq!(
            role_enum_to_emoji(&RoleEnum::Helminth),
            Some(Reaction::Helminth.emoji())
        );
    }

    #[test]
    fn test_role_enum_to_emoji_unknown() {
        assert_eq!(role_enum_to_emoji(&RoleEnum::Guest), None);
    }
}
