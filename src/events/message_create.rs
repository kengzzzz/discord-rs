use twilight_model::{
    channel::{Message, message::MessageType},
    id::Id,
};

use crate::{
    configs::discord::{CACHE, HTTP},
    dbs::mongo::{channel::ChannelEnum, role::RoleEnum},
    services::{
        broadcast::BroadcastService, channel::ChannelService, role::RoleService, spam::SpamService,
    },
    utils::embed,
};

pub async fn handle(message: Message) {
    if message.author.bot
        || message.author.system.unwrap_or(false)
        || message.kind != MessageType::Regular
    {
        return;
    }

    let Some(guild_id) = message.guild_id else {
        return;
    };

    let q_role = RoleService::get_by_type(guild_id.get(), &RoleEnum::Quarantine).await;
    let q_channel = ChannelService::get_by_type(guild_id.get(), &ChannelEnum::Quarantine).await;

    if let (Some(_), Some(channel)) = (q_role, q_channel) {
        if SpamService::is_quarantined(guild_id.get(), message.author.id.get()).await {
            let _ = HTTP.delete_message(message.channel_id, message.id).await;
            if let Some(token) =
                SpamService::get_token(guild_id.get(), message.author.id.get()).await
            {
                if let Some(guild_ref) = CACHE.guild(guild_id) {
                    if let Ok(embed) =
                        embed::quarantine_reminder_embed(&guild_ref, channel.channel_id, &token)
                    {
                        let channel_id = Id::new(channel.channel_id);
                        let _ = HTTP
                            .create_message(channel_id)
                            .content(&format!("<@{}>", message.author.id))
                            .embeds(&[embed])
                            .await;
                    }
                }
            }
            return;
        } else if let Some(token) = SpamService::log_message(guild_id.get(), &message).await {
            if let Some(guild_ref) = CACHE.guild(guild_id) {
                if let Ok(embeds) =
                    embed::quarantine_embed(&guild_ref, &message, channel.channel_id, &token)
                {
                    let channel_id = Id::new(channel.channel_id);
                    let _ = HTTP
                        .create_message(channel_id)
                        .content(&format!("<@{}>", message.author.id))
                        .embeds(&embeds)
                        .await;
                }
            }

            SpamService::quarantine_member(guild_id, message.author.id, &token).await;
            return;
        }
    }

    for channel in ChannelService::get(message.channel_id.get()).await {
        match channel.channel_type {
            ChannelEnum::Broadcast => {
                BroadcastService::handle(&message).await;
            }
            ChannelEnum::Quarantine => {}
            _ => {}
        }
    }
}
