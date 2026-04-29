use twilight_model::application::interaction::modal::{
    ModalInteractionComponent, ModalInteractionData,
};

pub fn modal_value_of<'a>(data: &'a ModalInteractionData, id: &str) -> Option<&'a str> {
    data.components
        .iter()
        .find_map(|component| modal_component_value_of(component, id))
}

fn modal_component_value_of<'a>(
    component: &'a ModalInteractionComponent,
    id: &str,
) -> Option<&'a str> {
    match component {
        ModalInteractionComponent::ActionRow(row) => row
            .components
            .iter()
            .find_map(|component| modal_component_value_of(component, id)),
        ModalInteractionComponent::Label(label) => modal_component_value_of(&label.component, id),
        ModalInteractionComponent::TextInput(input) if input.custom_id == id => Some(&input.value),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/modal.rs"]
mod tests;
