use std::slice;
use std::sync::Arc;

use std::collections::HashSet;
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::channel::message::{EmojiReactionType, Message};
use twilight_model::id::{Id, marker::GuildMarker};

use crate::{
    context::Context,
    dbs::mongo::models::role::RoleEnum,
    services::{channel::ChannelService, role::RoleService},
    utils::{embed, reaction::role_enum_to_emoji},
};

use super::storage;

pub async fn ensure_message(ctx: Arc<Context>, guild_id: Id<GuildMarker>) {
    let Some(channel) = ChannelService::get_by_type(
        ctx.clone(),
        guild_id.get(),
        &crate::dbs::mongo::models::channel::ChannelEnum::UpdateRole,
    )
    .await
    else {
        return;
    };

    let channel_id = Id::new(channel.channel_id);

    let mut existing_message: Option<(u64, Message)> = None;
    if let Some(record) = storage::get(ctx.clone(), guild_id.get()).await {
        if let Ok(resp) = ctx
            .http
            .message(channel_id, Id::new(record.message_id))
            .await
        {
            if let Ok(msg) = resp.model().await {
                existing_message = Some((record.message_id, msg));
            }
        }
    }

    let mut info = Vec::new();
    let roles = [
        RoleEnum::RivenSilver,
        RoleEnum::Helminth,
        RoleEnum::UmbralForma,
        RoleEnum::Eidolon,
    ];
    for role_type in roles.iter() {
        if let Some(role) = RoleService::get_by_type(ctx.clone(), guild_id.get(), role_type).await {
            if role.self_assignable {
                if let (Some(emoji), Some(role_ref)) = (
                    role_enum_to_emoji(role_type),
                    ctx.cache.role(Id::new(role.role_id)),
                ) {
                    info.push((role_ref.name.clone(), emoji));
                }
            }
        }
    }

    let mut embed_opt = None;
    let mut content_opt = None;
    if let Some(guild_ref) = ctx.cache.guild(guild_id) {
        if let Ok(embed) = embed::role_message_embed(&guild_ref, &info) {
            embed_opt = Some(embed);
        } else {
            content_opt = Some("กดอีโมจิเพื่อรับหรือลบ role");
        }
    }

    let embed_slice = embed_opt.as_ref().map(slice::from_ref);
    let content_opt = content_opt.as_ref().map(|e| *e);

    if let Some((msg_id, msg)) = existing_message {
        let mut correct = true;
        if let Some(embed) = embed_slice {
            correct &= msg.embeds.first() == Some(&(*embed)[0]);
            correct &= msg.embeds.len() == 1;
            correct &= msg.content.is_empty();
        } else if let Some(content) = content_opt {
            correct &= msg.content == content;
            correct &= msg.embeds.is_empty();
        }

        let expected: HashSet<&str> = info.iter().map(|(_, e)| *e).collect();
        let actual: HashSet<&str> = msg
            .reactions
            .iter()
            .filter_map(|r| match &r.emoji {
                EmojiReactionType::Unicode { name } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        correct &= expected == actual;

        if correct {
            return;
        }

        let mut update = ctx.http.update_message(channel_id, Id::new(msg_id));
        if let Some(embed) = embed_slice {
            update = update.embeds(Some(embed));
            update = update.content(None);
        } else if let Some(content) = content_opt {
            update = update.content(Some(content));
            update = update.embeds(None);
        }
        if update.await.is_ok() {
            if let Err(e) = ctx
                .http
                .delete_all_reactions(channel_id, Id::new(msg_id))
                .await
            {
                tracing::warn!(channel_id = channel_id.get(), message_id = msg_id, error = %e, "failed to clear reactions");
            }
            for (_, emoji) in &info {
                let reaction = RequestReactionType::Unicode { name: emoji };
                if let Err(e) = ctx
                    .http
                    .create_reaction(channel_id, Id::new(msg_id), &reaction)
                    .await
                {
                    tracing::warn!(channel_id = channel_id.get(), message_id = msg_id, error = %e, "failed to add reaction");
                }
            }
            storage::set(ctx.clone(), guild_id.get(), channel_id.get(), msg_id).await;
        }
        return;
    }
    let mut create = ctx.http.create_message(channel_id);
    if let Some(embed) = embed_slice {
        create = create.embeds(embed);
    } else if let Some(content) = content_opt {
        create = create.content(content);
    }

    if let Ok(response) = create.await {
        if let Ok(msg) = response.model().await {
            for (_, emoji) in &info {
                let reaction = RequestReactionType::Unicode { name: emoji };
                if let Err(e) = ctx
                    .http
                    .create_reaction(channel_id, msg.id, &reaction)
                    .await
                {
                    tracing::warn!(channel_id = channel_id.get(), message_id = msg.id.get(), error = %e, "failed to add reaction");
                }
            }
            storage::set(ctx.clone(), guild_id.get(), channel_id.get(), msg.id.get()).await;
        }
    }
}
