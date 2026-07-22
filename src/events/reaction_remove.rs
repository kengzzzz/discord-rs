use twilight_model::{gateway::payload::incoming::ReactionRemove, id::Id};

use crate::{
    context::Context,
    dbs::mongo::models::channel::ChannelEnum,
    services::{channel::ChannelService, role::RoleService, role_message},
    utils::reaction::emoji_to_role_enum,
};
use std::sync::Arc;

pub async fn handle(ctx: Arc<Context>, event: ReactionRemove) {
    let Some(guild_id) = event.guild_id else {
        return;
    };
    // No bot filter here (unlike reaction_add): MESSAGE_REACTION_REMOVE carries no member data,
    // and removing a self-assignable role from a bot that un-reacts is harmless anyway.
    let channels = ChannelService::get(&ctx, event.channel_id.get()).await;
    if !channels
        .iter()
        .any(|ch| ch.channel_type == ChannelEnum::UpdateRole)
    {
        return;
    }

    if let Some(record) = role_message::storage::get(&ctx, guild_id.get()).await {
        if record.message_id != event.message_id.get() {
            return;
        }
    } else {
        return;
    }

    let Some(role_type) = emoji_to_role_enum(&event.emoji) else {
        return;
    };

    if let Some(role) = RoleService::get_by_type(&ctx, guild_id.get(), &role_type).await
        && role.self_assignable
        && let Err(e) = ctx
            .http
            .remove_guild_member_role(guild_id, event.user_id, Id::new(role.role_id))
            .await
    {
        tracing::warn!(guild_id = guild_id.get(), user_id = event.user_id.get(), error = %e, "failed to remove role via reaction");
    }
}
