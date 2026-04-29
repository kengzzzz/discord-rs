use std::sync::Arc;
use twilight_gateway::Event;

use crate::{context::Context, features::registry};

pub async fn handle_interaction_fast(ctx: Arc<Context>, event: Event) {
    let Event::InteractionCreate(boxed) = event else {
        return;
    };
    registry()
        .handle_interaction(ctx, (*boxed).0)
        .await
}

pub async fn dispatch_event(ctx: Arc<Context>, event: Event) {
    registry()
        .dispatch_event(ctx, event)
        .await;
}
