use twilight_model::{gateway::payload::incoming::ReactionAdd, id::Id};

use crate::{
    context::Context,
    dbs::mongo::channel::ChannelEnum,
    services::{channel::ChannelService, role::RoleService, role_message::RoleMessageService},
    utils::reaction::emoji_to_role_enum,
};
use std::sync::Arc;

pub async fn handle(ctx: Arc<Context>, event: ReactionAdd) {
    let Some(guild_id) = event.guild_id else {
        return;
    };
    let is_bot = event
        .member
        .as_ref()
        .map(|m| m.user.bot || m.user.system.unwrap_or_default())
        .unwrap_or_default();
    if is_bot {
        return;
    }

    let channels = ChannelService::get(ctx.clone(), event.channel_id.get()).await;
    if !channels
        .iter()
        .any(|ch| ch.channel_type == ChannelEnum::UpdateRole)
    {
        return;
    }

    if let Some(record) = RoleMessageService::get(ctx.clone(), guild_id.get()).await {
        if record.message_id != event.message_id.get() {
            return;
        }
    } else {
        return;
    }

    let Some(role_type) = emoji_to_role_enum(&event.emoji) else {
        return;
    };
    if let Some(role) = RoleService::get_by_type(ctx.clone(), guild_id.get(), &role_type).await {
        if role.self_assignable {
            if let Err(e) = ctx
                .http
                .add_guild_member_role(guild_id, event.user_id, Id::new(role.role_id))
                .await
            {
                tracing::warn!(guild_id = guild_id.get(), user_id = event.user_id.get(), error = %e, "failed to assign role via reaction");
            }
        }
    }
}
