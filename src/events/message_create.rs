use twilight_model::{
    channel::{Attachment, Message, message::MessageType},
    id::Id,
};

use crate::{
    configs::discord::{CACHE, HTTP},
    dbs::mongo::{channel::ChannelEnum, role::RoleEnum},
    services::{
        ai::AiService, broadcast::BroadcastService, channel::ChannelService, role::RoleService,
        spam::SpamService,
    },
    utils::embed,
};

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn build_ai_input(content: &str, referenced: Option<&str>) -> String {
    let trimmed = content.trim();
    if let Some(r) = referenced {
        if r.is_empty() {
            return trimmed.to_string();
        }
        format!("Replying to: {r}\n{trimmed}")
    } else {
        trimmed.to_string()
    }
}

#[cfg_attr(test, allow(dead_code))]
const MAX_ATTACHMENTS: usize = 5;

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn collect_attachments(message: &Message) -> Vec<Attachment> {
    let mut list = message.attachments.clone();
    if let Some(ref_msg) = &message.referenced_message {
        list.extend(ref_msg.attachments.clone());
    }
    list.truncate(MAX_ATTACHMENTS);
    list
}

pub async fn handle(message: Message) {
    if message.author.bot
        || message.author.system.unwrap_or(false)
        || (message.kind != MessageType::Regular && message.kind != MessageType::Reply)
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
        if channel.channel_type == ChannelEnum::Broadcast {
            BroadcastService::handle(&message).await;
        }
    }

    if let Some(user) = &CACHE.current_user() {
        let bot_id = user.id;
        if message.mentions.iter().any(|m| m.id == bot_id) {
            let mention1 = format!("<@{}>", bot_id.get());
            let mention2 = format!("<@!{}>", bot_id.get());
            let mut content = message.content.replace(&mention1, "");
            content = content.replace(&mention2, "");
            let ref_text = message
                .referenced_message
                .as_deref()
                .map(|m| m.content.as_str());
            let input = build_ai_input(&content, ref_text);
            let attachments = collect_attachments(&message);
            if let Ok(reply) = AiService::handle_interaction(
                message.author.id,
                &message.author.name,
                &input,
                attachments,
            )
            .await
            {
                if let Ok(embeds) = embed::ai_embeds(&reply) {
                    let _ = HTTP
                        .create_message(message.channel_id)
                        .embeds(&embeds)
                        .await;
                }
            }
        }
    }
}
