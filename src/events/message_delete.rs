use twilight_model::gateway::payload::incoming::{MessageDelete, MessageDeleteBulk};

use crate::context::Context;
use crate::services::role_message::RoleMessageService;
use std::sync::Arc;

pub async fn handle_single(ctx: Arc<Context>, event: MessageDelete) {
    let Some(guild_id) = event.guild_id else {
        return;
    };

    if let Some(record) = RoleMessageService::get(ctx.clone(), guild_id.get()).await {
        if record.message_id == event.id.get() {
            RoleMessageService::ensure_message(ctx.clone(), guild_id).await;
        }
    }
}

pub async fn handle_bulk(ctx: Arc<Context>, event: MessageDeleteBulk) {
    let Some(guild_id) = event.guild_id else {
        return;
    };

    if let Some(record) = RoleMessageService::get(ctx.clone(), guild_id.get()).await {
        if event.ids.iter().any(|id| id.get() == record.message_id) {
            RoleMessageService::ensure_message(ctx.clone(), guild_id).await;
        }
    }
}
