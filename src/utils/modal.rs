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
