use std::slice;
use std::sync::Arc;

use twilight_http::request::channel::reaction::RequestReactionType;
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
                Some(EmbedFooter {
                    text: at,
                    icon_url: ai,
                    ..
                }),
                Some(EmbedFooter {
                    text: bt,
                    icon_url: bi,
                    ..
                }),
            ) => at == bt && ai == bi,
            (None, None) => true,
            _ => false,
        }
}

pub async fn ensure_message(ctx: Arc<Context>, guild_id: Id<GuildMarker>) {
    let Some(channel) = ChannelService::get_by_type(
        &ctx,
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

    let roles = [
        RoleEnum::RivenSilver,
        RoleEnum::Helminth,
        RoleEnum::UmbralForma,
        RoleEnum::Eidolon,
        RoleEnum::Live,
    ];
    let mut info = Vec::with_capacity(roles.len());
    for role_type in roles.iter() {
        if let Some(role) = RoleService::get_by_type(&ctx, guild_id.get(), role_type).await {
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

    let guild_ref = match ctx.cache.guild(guild_id) {
        Some(g) => g,
        None => {
            tracing::warn!(guild_id = guild_id.get(), "guild not found in cache");
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
        let mut correct = true;
        if let Some(existing) = msg.embeds.first() {
            correct &= embed_equals(existing, &embed_slice[0]);
            correct &= msg.embeds.len() == 1;
            correct &= msg.content.is_empty();
        } else {
            correct = false;
        }

        if correct {
            return;
        }

        let mut update = ctx.http.update_message(channel_id, Id::new(msg_id));
        update = update.embeds(Some(embed_slice));
        update = update.content(None);
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
    create = create.embeds(embed_slice);

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
