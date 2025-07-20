use super::*;
use twilight_model::application::interaction::modal::{
    ModalInteractionDataActionRow, ModalInteractionDataComponent,
};
use twilight_model::channel::message::component::ComponentType;

fn build_data() -> ModalInteractionData {
    ModalInteractionData {
        custom_id: "test".into(),
        components: vec![
            ModalInteractionDataActionRow {
                components: vec![ModalInteractionDataComponent {
                    custom_id: "a".into(),
                    kind: ComponentType::TextInput,
                    value: Some("1".into()),
                }],
            },
            ModalInteractionDataActionRow {
                components: vec![ModalInteractionDataComponent {
                    custom_id: "b".into(),
                    kind: ComponentType::TextInput,
                    value: None,
                }],
            },
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
