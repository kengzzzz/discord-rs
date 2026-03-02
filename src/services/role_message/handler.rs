use std::slice;
use std::sync::Arc;

use futures::future::join_all;
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::channel::message::EmojiReactionType;
use twilight_model::channel::message::{Embed, Message, embed::EmbedFooter};
use twilight_model::id::{Id, marker::GuildMarker};

use crate::{
    context::Context,
    dbs::mongo::models::role::RoleEnum,
    services::{channel::ChannelService, role::RoleService},
    utils::{embed, reaction::role_enum_to_emoji},
};

use super::storage;

fn embed_equals(a: &Embed, b: &Embed) -> bool {
    a.color == b.color
        && a.title == b.title
        && a.description == b.description
        && a.fields == b.fields
        && match (&a.footer, &b.footer) {
            (
                Some(EmbedFooter { text: at, icon_url: ai, .. }),
                Some(EmbedFooter { text: bt, icon_url: bi, .. }),
            ) => at == bt && ai == bi,
            (None, None) => true,
            _ => false,
        }
}

pub async fn ensure_message(ctx: &Arc<Context>, guild_id: Id<GuildMarker>) {
    let Some(channel) = ChannelService::get_by_type(
        ctx,
        guild_id.get(),
        &crate::dbs::mongo::models::channel::ChannelEnum::UpdateRole,
    )
    .await
    else {
        return;
    };

    let channel_id = Id::new(channel.channel_id);

    let mut existing_message: Option<(u64, Message)> = None;
    if let Some(record) = storage::get(ctx, guild_id.get()).await
        && let Ok(resp) = ctx
            .http
            .message(channel_id, Id::new(record.message_id))
            .await
        && let Ok(msg) = resp.model().await
    {
        existing_message = Some((record.message_id, msg));
    }

    let roles = [
        RoleEnum::RivenSilver,
        RoleEnum::Helminth,
        RoleEnum::UmbralForma,
        RoleEnum::Eidolon,
        RoleEnum::Live,
    ];
    let role_futures = roles
        .iter()
        .map(|role_type| async move {
            let role = RoleService::get_by_type(ctx, guild_id.get(), role_type).await;
            (role_type, role)
        });
    let role_results = join_all(role_futures).await;

    let mut info = Vec::with_capacity(roles.len());
    for (role_type, role_opt) in role_results {
        if let Some(role) = role_opt
            && role.self_assignable
            && let (Some(emoji), Some(role_ref)) = (
                role_enum_to_emoji(role_type),
                ctx.cache.role(Id::new(role.role_id)),
            )
        {
            info.push((role_ref.name.clone(), emoji.to_string()));
        }
    }

    let guild_ref = match ctx.cache.guild(guild_id) {
        Some(g) => g,
        None => {
            tracing::warn!(
                guild_id = guild_id.get(),
                "guild not found in cache"
            );
            return;
        }
    };

    let embed = match embed::role_message_embed(&guild_ref, &info) {
        Ok(embed) => embed,
        Err(e) => {
            tracing::error!(guild_id = guild_id.get(), error = %e, "failed to build role message embed");
            return;
        }
    };

    let embed_slice = slice::from_ref(&embed);

    if let Some((msg_id, msg)) = existing_message {
        let mut needs_embed_update = true;
        let mut target_reactions: Vec<String> = info
            .iter()
            .map(|(_, e)| e.clone())
            .collect();

        if let Some(existing_embed) = msg.embeds.first()
            && embed_equals(existing_embed, &embed_slice[0])
            && msg.embeds.len() == 1
            && msg.content.is_empty()
        {
            needs_embed_update = false;
        }

        let bot_reacted_emojis: Vec<String> = msg
            .reactions
            .iter()
            .filter(|r| r.me)
            .filter_map(|r| match &r.emoji {
                EmojiReactionType::Unicode { name } => Some(name.clone()),
                _ => None,
            })
            .collect();

        target_reactions.retain(|emoji| !bot_reacted_emojis.contains(emoji));

        if !needs_embed_update && target_reactions.is_empty() {
            return;
        }

        if needs_embed_update {
            let mut update = ctx
                .http
                .update_message(channel_id, Id::new(msg_id));
            update = update
                .embeds(Some(embed_slice))
                .content(None);
            if let Err(e) = update.await {
                tracing::error!(channel_id = channel_id.get(), message_id = msg_id, error = %e, "failed to update message");
                return;
            }
        }

        for emoji in target_reactions {
            let reaction = RequestReactionType::Unicode { name: &emoji };
            if let Err(e) = ctx
                .http
                .create_reaction(channel_id, Id::new(msg_id), &reaction)
                .await
            {
                tracing::warn!(channel_id = channel_id.get(), message_id = msg_id, error = %e, "failed to add reaction");
            }
        }
        return;
    }
    let mut create = ctx.http.create_message(channel_id);
    create = create.embeds(embed_slice);

    match create.await {
        Ok(response) => {
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
                storage::set(
                    ctx,
                    guild_id.get(),
                    channel_id.get(),
                    msg.id.get(),
                )
                .await;
            }
        }
        Err(e) => {
            tracing::error!(channel_id = channel_id.get(), error = %e, "failed to create role message");
        }
    }
}
