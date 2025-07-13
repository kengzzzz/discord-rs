pub mod ai;
pub mod broadcast;
pub mod quarantine;

use crate::context::Context;
use std::sync::Arc;
use twilight_model::{channel::Message, channel::message::MessageType};

pub async fn handle(ctx: Arc<Context>, message: Message) {
    if message.author.bot
        || message.author.system.unwrap_or(false)
        || (message.kind != MessageType::Regular && message.kind != MessageType::Reply)
    {
        return;
    }

    if quarantine::handle_quarantine(ctx.clone(), &message).await {
        return;
    }

    broadcast::handle_broadcast(ctx.clone(), &message).await;

    ai::handle_ai(ctx, &message).await;
}
