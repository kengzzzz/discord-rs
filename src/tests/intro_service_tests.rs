use twilight_model::application::interaction::modal::{
    ModalInteractionData, ModalInteractionDataActionRow, ModalInteractionDataComponent,
};
use twilight_model::channel::message::component::ComponentType;

use crate::services::introduction::{self};

fn build_modal(values: &[(&str, &str)]) -> ModalInteractionData {
    let rows = values
        .iter()
        .map(|(id, val)| ModalInteractionDataActionRow {
            components: vec![ModalInteractionDataComponent {
                custom_id: id.to_string(),
                kind: ComponentType::TextInput,
                value: Some((*val).to_string()),
            }],
        })
        .collect();
    ModalInteractionData {
        custom_id: "intro".to_string(),
        components: rows,
    }
}

#[test]
fn test_parse_modal_success() {
    let data = build_modal(&[
        ("name", "Alice"),
        ("age", "21"),
        ("ign", "IGN"),
        ("clan", "Clan"),
    ]);
    let details = introduction::parse_modal(&data).expect("parsed");
    assert_eq!(details.name, "Alice");
    assert_eq!(details.age, Some(21));
    assert_eq!(details.ign.as_deref(), Some("IGN"));
    assert_eq!(details.clan.as_deref(), Some("Clan"));
}

#[test]
fn test_parse_modal_missing_name() {
    let data = build_modal(&[("name", " "), ("age", "10")]);
    assert!(introduction::parse_modal(&data).is_none());
}
