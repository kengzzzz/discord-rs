use std::sync::Arc;
use twilight_model::application::interaction::Interaction;

use crate::{context::Context, features::registry};

pub async fn handle(ctx: Arc<Context>, interaction: Interaction) {
    registry()
        .handle_interaction(ctx, interaction)
        .await;
}
