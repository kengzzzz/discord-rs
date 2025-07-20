use twilight_model::application::interaction::modal::ModalInteractionData;

use crate::utils::modal::modal_value_of;

pub(crate) fn parse_modal(data: &ModalInteractionData) -> Option<String> {
    modal_value_of(data, "token")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
#[path = "tests/form.rs"]
mod tests;
