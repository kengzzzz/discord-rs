use super::*;
use twilight_model::application::interaction::modal::{
    ModalInteractionActionRow, ModalInteractionComponent, ModalInteractionData,
    ModalInteractionTextInput,
};

fn build_data() -> ModalInteractionData {
    ModalInteractionData {
        custom_id: "test".into(),
        resolved: None,
        components: vec![
            ModalInteractionComponent::ActionRow(ModalInteractionActionRow {
                id: 1,
                components: vec![ModalInteractionComponent::TextInput(ModalInteractionTextInput {
                    custom_id: "a".into(),
                    id: 11,
                    value: "1".into(),
                })],
            }),
            ModalInteractionComponent::ActionRow(ModalInteractionActionRow {
                id: 2,
                components: vec![ModalInteractionComponent::TextInput(ModalInteractionTextInput {
                    custom_id: "b".into(),
                    id: 12,
                    value: String::new(),
                })],
            }),
        ],
    }
}

#[test]
fn finds_value_for_id() {
    let data = build_data();
    assert_eq!(modal_value_of(&data, "a"), Some("1"));
}

#[test]
fn returns_none_when_absent() {
    let data = build_data();
    assert_eq!(modal_value_of(&data, "missing"), None);
}
