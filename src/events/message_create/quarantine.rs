use crate::{
    context::Context,
    dbs::mongo::models::{channel::ChannelEnum, role::RoleEnum},
    services::{
        channel::ChannelService,
        role::RoleService,
        spam::{self, SpamService},
    },
};
use std::sync::Arc;
use twilight_model::{channel::Message, id::Id};

pub async fn handle_quarantine(ctx: Arc<Context>, message: &Message) -> bool {
    let Some(guild_id) = message.guild_id else {
        return false;
    };

    let q_role = RoleService::get_by_type(ctx.clone(), guild_id.get(), &RoleEnum::Quarantine).await;
    let q_channel =
        ChannelService::get_by_type(ctx.clone(), guild_id.get(), &ChannelEnum::Quarantine).await;

    if let (Some(_), Some(channel)) = (q_role, q_channel) {
        if SpamService::is_quarantined(ctx.clone(), guild_id.get(), message.author.id.get()).await {
            if let Err(e) = ctx
                .http
                .delete_message(message.channel_id, message.id)
                .await
            {
                tracing::warn!(
                    channel_id = message.channel_id.get(),
                    message_id = message.id.get(),
                    error = %e,
                    "failed to delete message from quarantined user"
                );
            }
            if let Some(token) = crate::services::spam::quarantine::get_token(
                ctx.clone(),
                guild_id.get(),
                message.author.id.get(),
            )
            .await
            {
                if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                    if let Ok(embed) = spam::embed::quarantine_reminder_embed(
                        &guild_ref,
                        channel.channel_id,
                        &token,
                    ) {
                        let channel_id = Id::new(channel.channel_id);
                        if let Err(e) = ctx
                            .http
                            .create_message(channel_id)
                            .content(&format!("<@{}>", message.author.id))
                            .embeds(&[embed])
                            .await
                        {
                            tracing::warn!(
                                channel_id = channel_id.get(),
                                user_id = message.author.id.get(),
                                error = %e,
                                "failed to send quarantine reminder"
                            );
                        }
                    }
                }
            }
            return true;
        } else if let Some(token) =
            crate::services::spam::log::log_message(ctx.clone(), guild_id.get(), message).await
        {
            if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                if let Ok(embeds) =
                    spam::embed::quarantine_embed(&guild_ref, message, channel.channel_id, &token)
                {
                    let channel_id = Id::new(channel.channel_id);
                    if let Err(e) = ctx
                        .http
                        .create_message(channel_id)
                        .content(&format!("<@{}>", message.author.id))
                        .embeds(&embeds)
                        .await
                    {
                        tracing::warn!(
                            channel_id = channel_id.get(),
                            user_id = message.author.id.get(),
                            error = %e,
                            "failed to send quarantine notice"
                        );
                    }
                }
            }

            crate::services::spam::quarantine::quarantine_member(
                ctx.clone(),
                guild_id,
                message.author.id,
                &token,
            )
            .await;
            return true;
        }
    }

    false
}
