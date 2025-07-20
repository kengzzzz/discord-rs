use super::*;
use twilight_model::application::interaction::modal::{
    ModalInteractionDataActionRow, ModalInteractionDataComponent,
};
use twilight_model::channel::message::component::ComponentType;

fn build_data(
    name: Option<&str>,
    age: Option<&str>,
    ign: Option<&str>,
    clan: Option<&str>,
) -> ModalInteractionData {
    ModalInteractionData {
        custom_id: "intro".into(),
        components: vec![
            ModalInteractionDataActionRow {
                components: vec![ModalInteractionDataComponent {
                    custom_id: "name".into(),
                    kind: ComponentType::TextInput,
                    value: name.map(|v| v.to_string()),
                }],
            },
            ModalInteractionDataActionRow {
                components: vec![ModalInteractionDataComponent {
                    custom_id: "age".into(),
                    kind: ComponentType::TextInput,
                    value: age.map(|v| v.to_string()),
                }],
            },
            ModalInteractionDataActionRow {
                components: vec![ModalInteractionDataComponent {
                    custom_id: "ign".into(),
                    kind: ComponentType::TextInput,
                    value: ign.map(|v| v.to_string()),
                }],
            },
            ModalInteractionDataActionRow {
                components: vec![ModalInteractionDataComponent {
                    custom_id: "clan".into(),
                    kind: ComponentType::TextInput,
                    value: clan.map(|v| v.to_string()),
                }],
            },
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
