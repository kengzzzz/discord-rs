use super::*;
use twilight_model::application::interaction::modal::{
    ModalInteractionData, ModalInteractionDataActionRow, ModalInteractionDataComponent,
};
use twilight_model::channel::message::component::ComponentType;

fn build_data(value: Option<&str>) -> ModalInteractionData {
    ModalInteractionData {
        custom_id: "test".into(),
        components: vec![ModalInteractionDataActionRow {
            components: vec![ModalInteractionDataComponent {
                custom_id: "token".into(),
                kind: ComponentType::TextInput,
                value: value.map(|v| v.into()),
            }],
        }],
    }
}

#[test]
fn test_parse_modal_trimmed() {
    let data = build_data(Some("  token  "));
    assert_eq!(parse_modal(&data), Some("token".to_string()));
}

#[test]
fn test_parse_modal_empty() {
    let data = build_data(Some("   "));
    assert_eq!(parse_modal(&data), None);
}
