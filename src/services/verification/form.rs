use twilight_model::application::interaction::modal::ModalInteractionData;

pub(crate) fn value_of<'a>(data: &'a ModalInteractionData, id: &str) -> Option<&'a str> {
    for row in &data.components {
        for comp in &row.components {
            if comp.custom_id == id {
                return comp.value.as_deref();
            }
        }
    }
    None
}

pub(crate) fn parse_modal(data: &ModalInteractionData) -> Option<String> {
    value_of(data, "token")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}
