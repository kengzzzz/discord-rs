use twilight_model::application::interaction::modal::ModalInteractionData;

pub struct IntroDetails {
    pub name: String,
    pub age: Option<u8>,
    pub ign: Option<String>,
    pub clan: Option<String>,
}

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

pub(crate) fn parse_modal(data: &ModalInteractionData) -> Option<IntroDetails> {
    let name = value_of(data, "name")?.trim().to_owned();
    if name.is_empty() {
        return None;
    }

    let age = value_of(data, "age")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<u8>().ok());

    let ign = value_of(data, "ign")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned);

    let clan = value_of(data, "clan")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned);

    Some(IntroDetails {
        name,
        age,
        ign,
        clan,
    })
}
