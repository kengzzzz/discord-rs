use twilight_model::application::interaction::modal::ModalInteractionData;

use crate::utils::modal::modal_value_of;

pub const NAME_MAX_CHARS: usize = 100;
pub const IGN_MAX_CHARS: usize = 50;
pub const CLAN_MAX_CHARS: usize = 50;

pub struct IntroDetails {
    pub name: String,
    pub age: Option<u8>,
    pub ign: Option<String>,
    pub clan: Option<String>,
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

pub(crate) fn parse_modal(data: &ModalInteractionData) -> Option<IntroDetails> {
    let name = truncate(
        modal_value_of(data, "name")?.trim(),
        NAME_MAX_CHARS,
    );
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
        .map(|v| truncate(v, IGN_MAX_CHARS));

    let clan = modal_value_of(data, "clan")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| truncate(v, CLAN_MAX_CHARS));

    Some(IntroDetails { name, age, ign, clan })
}

#[cfg(test)]
#[path = "tests/form.rs"]
mod tests;
