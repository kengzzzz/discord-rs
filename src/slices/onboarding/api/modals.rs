use std::sync::Arc;

use twilight_model::application::interaction::{Interaction, modal::ModalInteractionData};

use crate::{context::Context, features::intro, features::verification};

pub async fn handle_intro_modal(
    ctx: Arc<Context>,
    interaction: Interaction,
    data: ModalInteractionData,
) {
    intro::modal::handle_intro_modal(ctx, interaction, data).await;
}

pub async fn handle_verify_modal(
    ctx: Arc<Context>,
    interaction: Interaction,
    data: ModalInteractionData,
) {
    verification::modal::handle_verify_modal(ctx, interaction, data).await;
}
