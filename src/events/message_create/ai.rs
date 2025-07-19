use twilight_model::{
    channel::{Attachment, Message},
    id::{Id, marker::UserMarker},
};

use crate::{
    context::Context,
    services::ai::{AiInteraction, AiService},
};
use std::{borrow::Cow, sync::Arc};

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

const MAX_ATTACHMENTS: usize = 5;

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

pub fn strip_mention<'a>(raw: &'a str, id: Id<UserMarker>) -> Cow<'a, str> {
    let bot_id = id.get();
    let bytes = raw.as_bytes();
    let len = bytes.len();
    if !raw.as_bytes().windows(2).any(|w| w == b"<@") {
        return Cow::Borrowed(raw);
    }
    let mut out: Option<String> = None;
    let mut last_copy = 0;
    let mut i = 0;
    while i < len {
        if bytes[i] == b'<' && i + 1 < len && bytes[i + 1] == b'@' {
            let mut j = i + 2;
            if j < len && bytes[j] == b'!' {
                j += 1;
            }
            let mut n: u64 = 0;
            let mut has_digit = false;
            while j < len {
                let b = bytes[j];
                if b.is_ascii_digit() {
                    has_digit = true;
                    n = n.saturating_mul(10).saturating_add((b - b'0') as u64);
                    j += 1;
                } else {
                    break;
                }
            }
            if has_digit && j < len && bytes[j] == b'>' && n == bot_id {
                let buf = out.get_or_insert_with(|| String::with_capacity(raw.len()));
                buf.push_str(&raw[last_copy..i]);
                last_copy = j + 1;
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }

    if let Some(mut buf) = out {
        if last_copy < len {
            buf.push_str(&raw[last_copy..]);
        }
        Cow::Owned(buf)
    } else {
        Cow::Borrowed(raw)
    }
}

pub async fn handle_ai(ctx: &Arc<Context>, message: &Message) {
    if let Some(user) = ctx.cache.current_user() {
        if message.mentions.iter().any(|m| m.id == user.id) {
            if let Err(e) = ctx.http.create_typing_trigger(message.channel_id).await {
                tracing::warn!(channel_id = message.channel_id.get(), error = %e, "failed to trigger typing");
            }
            if let Some(wait) = AiService::check_rate_limit(ctx, message.author.id).await {
                if let Ok(embed) = AiService::rate_limit_embed(wait) {
                    if let Err(e) = ctx
                        .http
                        .create_message(message.channel_id)
                        .embeds(&[embed])
                        .await
                    {
                        tracing::warn!(
                            channel_id = message.channel_id.get(),
                            user_id = message.author.id.get(),
                            error = %e,
                            "failed to send rate limit message",
                        );
                    }
                }
                return;
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
            let (attachments, ref_attachments) = collect_attachments(message);
            match AiService::handle_interaction(
                ctx,
                AiInteraction {
                    user_id: message.author.id,
                    user_name: &message.author.name,
                    message: input.as_ref(),
                    attachments,
                    ref_text: ref_text_opt,
                    ref_attachments,
                    ref_author,
                },
            )
            .await
            {
                Ok(reply) => {
                    if let Ok(embeds) = AiService::ai_embeds(&reply) {
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
                Err(e) => {
                    tracing::warn!(
                        channel_id = message.channel_id.get(),
                        error = %e,
                        "failed to handle AI interaction"
                    );
                }
            }
        }
    }
}
