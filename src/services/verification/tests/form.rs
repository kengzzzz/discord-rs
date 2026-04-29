use super::*;
use twilight_model::application::interaction::modal::{
    ModalInteractionActionRow, ModalInteractionComponent, ModalInteractionData,
    ModalInteractionTextInput,
};

fn build_data(value: Option<&str>) -> ModalInteractionData {
    ModalInteractionData {
        custom_id: "test".into(),
        resolved: None,
        components: vec![ModalInteractionComponent::ActionRow(ModalInteractionActionRow {
            id: 1,
            components: vec![ModalInteractionComponent::TextInput(ModalInteractionTextInput {
                custom_id: "token".into(),
                id: 11,
                value: value.unwrap_or_default().into(),
            })],
        })],
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
