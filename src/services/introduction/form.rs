use twilight_model::application::interaction::modal::ModalInteractionData;

use crate::utils::modal::modal_value_of;

pub struct IntroDetails {
    pub name: String,
    pub age: Option<u8>,
    pub ign: Option<String>,
    pub clan: Option<String>,
}

pub(crate) fn parse_modal(data: &ModalInteractionData) -> Option<IntroDetails> {
    let name = modal_value_of(data, "name")?
        .trim()
        .to_owned();
    if name.is_empty() {
        return None;
    }

    let age = modal_value_of(data, "age")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<u8>().ok());

    let ign = modal_value_of(data, "ign")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned);

    let clan = modal_value_of(data, "clan")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned);

    Some(IntroDetails { name, age, ign, clan })
}

#[cfg(test)]
#[path = "tests/form.rs"]
mod tests;
