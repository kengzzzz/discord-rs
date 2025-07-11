use once_cell::sync::OnceCell;
use regex::Regex;
use twilight_model::{
    channel::{Attachment, Message, message::MessageType},
    id::{Id, marker::UserMarker},
};

use crate::{
    context::Context,
    dbs::mongo::{channel::ChannelEnum, role::RoleEnum},
    services::{
        ai::AiService, broadcast::BroadcastService, channel::ChannelService, role::RoleService,
        spam::SpamService,
    },
    utils::embed,
};
use std::sync::Arc;

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

static BOT_MENTION_RE: OnceCell<Regex> = OnceCell::new();

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn collect_attachments(message: &Message) -> Vec<Attachment> {
    let mut list = message.attachments.clone();
    if let Some(ref_msg) = &message.referenced_message {
        list.extend(ref_msg.attachments.clone());
    }
    list.truncate(MAX_ATTACHMENTS);
    list
}

fn strip_mention(raw: &str, id: Id<UserMarker>) -> String {
    let re = BOT_MENTION_RE.get_or_init(|| {
        let id = id.get();
        let pattern = format!(r"<@!?(?:{id})>");
        Regex::new(&pattern).expect("failed to compile bot mention regex")
    });

    re.replace_all(raw, "").into_owned()
}

pub async fn handle(ctx: Arc<Context>, message: Message) {
    if message.author.bot
        || message.author.system.unwrap_or(false)
        || (message.kind != MessageType::Regular && message.kind != MessageType::Reply)
    {
        return;
    }

    let Some(guild_id) = message.guild_id else {
        return;
    };

    let q_role = RoleService::get_by_type(ctx.clone(), guild_id.get(), &RoleEnum::Quarantine).await;
    let q_channel =
        ChannelService::get_by_type(ctx.clone(), guild_id.get(), &ChannelEnum::Quarantine).await;

    if let (Some(_), Some(channel)) = (q_role, q_channel) {
        if SpamService::is_quarantined(ctx.clone(), guild_id.get(), message.author.id.get()).await {
            let _ = ctx
                .http
                .delete_message(message.channel_id, message.id)
                .await;
            if let Some(token) =
                SpamService::get_token(ctx.clone(), guild_id.get(), message.author.id.get()).await
            {
                if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                    if let Ok(embed) =
                        embed::quarantine_reminder_embed(&guild_ref, channel.channel_id, &token)
                    {
                        let channel_id = Id::new(channel.channel_id);
                        let _ = ctx
                            .http
                            .create_message(channel_id)
                            .content(&format!("<@{}>", message.author.id))
                            .embeds(&[embed])
                            .await;
                    }
                }
            }
            return;
        } else if let Some(token) =
            SpamService::log_message(ctx.clone(), guild_id.get(), &message).await
        {
            if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                if let Ok(embeds) =
                    embed::quarantine_embed(&guild_ref, &message, channel.channel_id, &token)
                {
                    let channel_id = Id::new(channel.channel_id);
                    let _ = ctx
                        .http
                        .create_message(channel_id)
                        .content(&format!("<@{}>", message.author.id))
                        .embeds(&embeds)
                        .await;
                }
            }

            SpamService::quarantine_member(ctx.clone(), guild_id, message.author.id, &token).await;
            return;
        }
    }

    for channel in ChannelService::get(ctx.clone(), message.channel_id.get()).await {
        if channel.channel_type == ChannelEnum::Broadcast {
            BroadcastService::handle(ctx.clone(), &message).await;
        }
    }

    if let Some(user) = &ctx.cache.current_user() {
        if message.mentions.iter().any(|m| m.id == user.id) {
            let _ = ctx.http.create_typing_trigger(message.channel_id).await;
            let content = strip_mention(&message.content, user.id);
            let ref_text = message
                .referenced_message
                .as_deref()
                .map(|m| m.content.as_str());
            let input = build_ai_input(&content, ref_text);
            let attachments = collect_attachments(&message);
            if let Ok(reply) = AiService::handle_interaction(
                ctx.clone(),
                message.author.id,
                &message.author.name,
                &input,
                attachments,
            )
            .await
            {
                if let Ok(embeds) = embed::ai_embeds(&reply) {
                    let _ = ctx
                        .http
                        .create_message(message.channel_id)
                        .embeds(&embeds)
                        .await;
                }
            }
        }
    }
}
