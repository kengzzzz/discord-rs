use twilight_model::gateway::payload::incoming::{MessageDelete, MessageDeleteBulk};

use crate::services::role_message::RoleMessageService;

pub async fn handle_single(event: MessageDelete) {
    let Some(guild_id) = event.guild_id else {
        return;
    };

    if let Some(record) = RoleMessageService::get(guild_id.get()).await {
        if record.message_id == event.id.get() {
            RoleMessageService::ensure_message(guild_id).await;
        }
    }
}

pub async fn handle_bulk(event: MessageDeleteBulk) {
    let Some(guild_id) = event.guild_id else {
        return;
    };

    if let Some(record) = RoleMessageService::get(guild_id.get()).await {
        if event.ids.iter().any(|id| id.get() == record.message_id) {
            RoleMessageService::ensure_message(guild_id).await;
        }
    }
}
