use super::*;
use twilight_model::id::{Id, marker::EmojiMarker};

#[test]
fn test_emoji_to_role_enum_known() {
    let emoji = EmojiReactionType::Unicode { name: Reaction::Riven.emoji().to_string() };

    assert_eq!(
        emoji_to_role_enum(&emoji),
        Some(RoleEnum::RivenSilver)
    );
}

#[test]
fn test_emoji_to_role_enum_unknown_unicode() {
    let emoji = EmojiReactionType::Unicode { name: "❓".to_string() };

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
