use twilight_model::gateway::payload::incoming::{MessageDelete, MessageDeleteBulk};

use crate::context::Context;
use crate::services::role_message;
use std::sync::Arc;

pub async fn handle_single(ctx: Arc<Context>, event: MessageDelete) {
    let Some(guild_id) = event.guild_id else {
        return;
    };

    if let Some(record) = role_message::storage::get(&ctx, guild_id.get()).await {
        if record.message_id == event.id.get() {
            role_message::handler::ensure_message(&ctx, guild_id).await;
        }
    }
}

pub async fn handle_bulk(ctx: Arc<Context>, event: MessageDeleteBulk) {
    let Some(guild_id) = event.guild_id else {
        return;
    };

    if let Some(record) = role_message::storage::get(&ctx, guild_id.get()).await {
        if event
            .ids
            .iter()
            .any(|id| id.get() == record.message_id)
        {
            role_message::handler::ensure_message(&ctx, guild_id).await;
        }
    }
}
