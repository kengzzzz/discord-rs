use twilight_model::application::interaction::modal::ModalInteractionData;

pub fn modal_value_of<'a>(data: &'a ModalInteractionData, id: &str) -> Option<&'a str> {
    for row in &data.components {
        for comp in &row.components {
            if comp.custom_id == id {
                return comp.value.as_deref();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
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
}
