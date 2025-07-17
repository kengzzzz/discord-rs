use super::form;
use twilight_model::application::interaction::modal::{
    ModalInteractionData, ModalInteractionDataActionRow, ModalInteractionDataComponent,
};
use twilight_model::channel::message::component::ComponentType;

fn build_modal(value: &str) -> ModalInteractionData {
    let row = ModalInteractionDataActionRow {
        components: vec![ModalInteractionDataComponent {
            custom_id: "token".to_string(),
            kind: ComponentType::TextInput,
            value: Some(value.to_string()),
        }],
    };
    ModalInteractionData {
        custom_id: "verify".to_string(),
        components: vec![row],
    }
}

#[test]
fn test_parse_modal_success() {
    let data = build_modal("abc");
    let token = form::parse_modal(&data).expect("parsed");
    assert_eq!(token, "abc");
}

#[test]
fn test_parse_modal_empty() {
    let data = build_modal(" ");
    assert!(form::parse_modal(&data).is_none());
}
