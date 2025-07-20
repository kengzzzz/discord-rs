use std::sync::Arc;
use twilight_gateway::Event;

use crate::{
    context::Context,
    events::{
        interaction_create, member_add, member_remove, message_create, message_delete,
        reaction_add, reaction_remove, ready,
    },
};

pub async fn handle_interaction_fast(ctx: Arc<Context>, event: Event) {
    let Event::InteractionCreate(boxed) = event else {
        return;
    };
    interaction_create::handle(ctx, (*boxed).0).await
}

pub async fn dispatch_event(ctx: Arc<Context>, event: Event) {
    match event {
        Event::MessageCreate(boxed) => message_create::handle(ctx, (*boxed).0).await,
        Event::Ready(boxed) => ready::handle(ctx, *boxed).await,
        Event::MemberAdd(boxed) => member_add::handle(ctx, *boxed).await,
        Event::MemberRemove(event) => member_remove::handle(ctx, event).await,
        Event::ReactionAdd(boxed) => reaction_add::handle(ctx, *boxed).await,
        Event::ReactionRemove(boxed) => reaction_remove::handle(ctx, *boxed).await,
        Event::MessageDelete(event) => message_delete::handle_single(ctx, event).await,
        Event::MessageDeleteBulk(event) => message_delete::handle_bulk(ctx, event).await,
        _ => {}
    }
}
