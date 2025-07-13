use once_cell::sync::OnceCell;
use regex::Regex;
use twilight_model::{
    channel::{Attachment, Message, message::MessageType},
    id::{Id, marker::UserMarker},
};

use crate::{
    context::Context,
    dbs::mongo::models::{channel::ChannelEnum, role::RoleEnum},
    services::{
        ai::AiService, broadcast::BroadcastService, channel::ChannelService, role::RoleService,
        spam::SpamService,
    },
    utils::embed,
};
use std::{borrow::Cow, sync::Arc};

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn build_ai_input<'a>(content: &'a str, referenced: Option<&'a str>) -> Cow<'a, str> {
    let trimmed = content.trim();
    if let Some(r) = referenced {
        if r.is_empty() {
            Cow::Borrowed(trimmed)
        } else {
            Cow::Owned(format!("Replying to: {r}\n{trimmed}"))
        }
    } else {
        Cow::Borrowed(trimmed)
    }
}

#[cfg_attr(test, allow(dead_code))]
const MAX_ATTACHMENTS: usize = 5;

static BOT_MENTION_RE: OnceCell<Regex> = OnceCell::new();

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn collect_attachments(message: &Message) -> (Vec<Attachment>, Vec<Attachment>) {
    let mut main = message.attachments.clone();
    main.truncate(MAX_ATTACHMENTS);

    let remaining = MAX_ATTACHMENTS.saturating_sub(main.len());
    let mut refs = Vec::new();
    if remaining > 0 {
        if let Some(ref_msg) = &message.referenced_message {
            refs = ref_msg.attachments.clone();
            refs.truncate(remaining);
        }
    }

    (main, refs)
}

pub(crate) fn strip_mention<'a>(raw: &'a str, id: Id<UserMarker>) -> Cow<'a, str> {
    let re = BOT_MENTION_RE.get_or_init(|| {
        let id = id.get();
        let pattern = format!(r"<@!?(?:{id})>");
        Regex::new(&pattern).expect("failed to compile bot mention regex")
    });

    re.replace_all(raw, "")
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
            if let Err(e) = ctx
                .http
                .delete_message(message.channel_id, message.id)
                .await
            {
                tracing::warn!(channel_id = message.channel_id.get(), message_id = message.id.get(), error = %e, "failed to delete message from quarantined user");
            }
            if let Some(token) =
                SpamService::get_token(ctx.clone(), guild_id.get(), message.author.id.get()).await
            {
                if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                    if let Ok(embed) =
                        embed::quarantine_reminder_embed(&guild_ref, channel.channel_id, &token)
                    {
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
            return;
        } else if let Some(token) =
            SpamService::log_message(ctx.clone(), guild_id.get(), &message).await
        {
            if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                if let Ok(embeds) =
                    embed::quarantine_embed(&guild_ref, &message, channel.channel_id, &token)
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
            if let Err(e) = ctx.http.create_typing_trigger(message.channel_id).await {
                tracing::warn!(channel_id = message.channel_id.get(), error = %e, "failed to trigger typing");
            }
            let content = strip_mention(&message.content, user.id);
            let ref_text_opt = message
                .referenced_message
                .as_ref()
                .map(|m| (*m.content).as_ref());
            let ref_author = message
                .referenced_message
                .as_ref()
                .map(|m| (*m.author.name).as_ref());
            let input = build_ai_input(content.as_ref(), ref_text_opt);
            let (attachments, ref_attachments) = collect_attachments(&message);
            if let Ok(reply) = AiService::handle_interaction(
                ctx.clone(),
                message.author.id,
                &message.author.name,
                input.as_ref(),
                attachments,
                ref_text_opt,
                ref_attachments,
                ref_author,
            )
            .await
            {
                if let Ok(embeds) = embed::ai_embeds(&reply) {
                    for embed in embeds {
                        if let Err(e) = ctx
                            .http
                            .create_message(message.channel_id)
                            .embeds(&[embed])
                            .await
                        {
                            tracing::warn!(
                                channel_id = message.channel_id.get(),
                                error = %e,
                                "failed to send AI response"
                            );
                        }
                    }
                }
            }
        }
    }
}
