use super::*;
use twilight_model::application::interaction::modal::{
    ModalInteractionActionRow, ModalInteractionComponent, ModalInteractionData,
    ModalInteractionTextInput,
};

fn build_data(
    name: Option<&str>,
    age: Option<&str>,
    ign: Option<&str>,
    clan: Option<&str>,
) -> ModalInteractionData {
    ModalInteractionData {
        custom_id: "intro".into(),
        resolved: None,
        components: vec![
            ModalInteractionComponent::ActionRow(ModalInteractionActionRow {
                id: 1,
                components: vec![ModalInteractionComponent::TextInput(ModalInteractionTextInput {
                    custom_id: "name".into(),
                    id: 11,
                    value: name.unwrap_or_default().to_string(),
                })],
            }),
            ModalInteractionComponent::ActionRow(ModalInteractionActionRow {
                id: 2,
                components: vec![ModalInteractionComponent::TextInput(ModalInteractionTextInput {
                    custom_id: "age".into(),
                    id: 12,
                    value: age.unwrap_or_default().to_string(),
                })],
            }),
            ModalInteractionComponent::ActionRow(ModalInteractionActionRow {
                id: 3,
                components: vec![ModalInteractionComponent::TextInput(ModalInteractionTextInput {
                    custom_id: "ign".into(),
                    id: 13,
                    value: ign.unwrap_or_default().to_string(),
                })],
            }),
            ModalInteractionComponent::ActionRow(ModalInteractionActionRow {
                id: 4,
                components: vec![ModalInteractionComponent::TextInput(ModalInteractionTextInput {
                    custom_id: "clan".into(),
                    id: 14,
                    value: clan.unwrap_or_default().to_string(),
                })],
            }),
        ],
    }
}

#[test]
fn test_parse_modal_success() {
    let data = build_data(
        Some(" Alice "),
        Some(" 21 "),
        Some(" IGN "),
        Some(" Clan "),
    );

    let result = parse_modal(&data).unwrap();
    assert_eq!(result.name, "Alice");
    assert_eq!(result.age, Some(21));
    assert_eq!(result.ign.as_deref(), Some("IGN"));
    assert_eq!(result.clan.as_deref(), Some("Clan"));
}

#[test]
fn test_parse_modal_ignores_empty() {
    let data = build_data(Some("Bob"), Some(""), Some(" "), None);

    let result = parse_modal(&data).unwrap();
    assert_eq!(result.name, "Bob");
    assert!(result.age.is_none());
    assert!(result.ign.is_none());
    assert!(result.clan.is_none());
}

#[test]
fn test_parse_modal_requires_name() {
    let missing = build_data(None, Some("12"), None, None);
    assert!(parse_modal(&missing).is_none());

    let empty = build_data(Some("   "), Some("10"), None, None);
    assert!(parse_modal(&empty).is_none());
}

#[test]
fn test_parse_modal_truncates_overlong_fields() {
    let long_name = "A".repeat(500);
    let long_ign = "I".repeat(500);
    let long_clan = "C".repeat(500);
    let data = build_data(
        Some(&long_name),
        Some("21"),
        Some(&long_ign),
        Some(&long_clan),
    );

    let result = parse_modal(&data).unwrap();
    assert_eq!(result.name.chars().count(), NAME_MAX_CHARS);
    assert_eq!(
        result
            .ign
            .as_deref()
            .unwrap()
            .chars()
            .count(),
        IGN_MAX_CHARS
    );
    assert_eq!(
        result
            .clan
            .as_deref()
            .unwrap()
            .chars()
            .count(),
        CLAN_MAX_CHARS
    );
}
