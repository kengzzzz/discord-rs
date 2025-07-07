use mongodb::bson::doc;
use std::slice;

use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::id::{Id, marker::GuildMarker};

use crate::{
    configs::{
        CACHE_PREFIX,
        discord::{CACHE, HTTP},
    },
    dbs::{
        mongo::{
            channel::ChannelEnum, mongodb::MongoDB, role::RoleEnum, role_message::RoleMessage,
        },
        redis::{redis_delete, redis_get, redis_set},
    },
    services::{channel::ChannelService, role::RoleService},
    utils::{embed, reaction::role_enum_to_emoji},
};

pub struct RoleMessageService;

impl RoleMessageService {
    pub async fn get(guild_id: u64) -> Option<RoleMessage> {
        let redis_key = format!("{CACHE_PREFIX}:role-message:{guild_id}");

        if let Some(msg) = redis_get(&redis_key).await {
            return Some(msg);
        }

        if let Ok(Some(msg)) = MongoDB::get()
            .role_messages
            .find_one(doc! {"guild_id": guild_id as i64})
            .await
        {
            redis_set(&redis_key, &msg).await;
            return Some(msg);
        }

        None
    }

    pub async fn set(guild_id: u64, channel_id: u64, message_id: u64) {
        let _ = MongoDB::get()
            .role_messages
            .update_one(
                doc! {"guild_id": guild_id as i64},
                doc! {"$set": {"guild_id": guild_id as i64, "channel_id": channel_id as i64, "message_id": message_id as i64}},
            ).upsert(true)
            .await;
    }

    pub async fn purge_cache(guild_id: u64) {
        let redis_key = format!("{CACHE_PREFIX}:role-message:{guild_id}");
        redis_delete(&redis_key).await;
    }

    pub async fn ensure_message(guild_id: Id<GuildMarker>) {
        let Some(channel) =
            ChannelService::get_by_type(guild_id.get(), &ChannelEnum::UpdateRole).await
        else {
            return;
        };

        let channel_id = Id::new(channel.channel_id);

        let mut existing_message = None;
        if let Some(record) = Self::get(guild_id.get()).await {
            if HTTP
                .message(channel_id, Id::new(record.message_id))
                .await
                .is_ok()
            {
                existing_message = Some(record.message_id);
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
            if let Some(role) = RoleService::get_by_type(guild_id.get(), role_type).await {
                if role.self_assignable {
                    if let (Some(emoji), Some(role_ref)) = (
                        role_enum_to_emoji(role_type),
                        CACHE.role(Id::new(role.role_id)),
                    ) {
                        info.push((role_ref.name.clone(), emoji));
                    }
                }
            }
        }

        let mut embed_opt = None;
        let mut content_opt = None;
        if let Some(guild_ref) = CACHE.guild(guild_id) {
            if let Ok(embed) = embed::role_message_embed(&guild_ref, &info) {
                embed_opt = Some(embed);
            } else {
                content_opt = Some("กดอีโมจิเพื่อรับหรือลบ role");
            }
        }

        let embed_slice = embed_opt.as_ref().map(slice::from_ref);
        let content_opt = content_opt.as_ref().map(|e| *e);

        if let Some(msg_id) = existing_message {
            let mut update = HTTP.update_message(channel_id, Id::new(msg_id));
            if let Some(embed) = embed_slice {
                update = update.embeds(Some(embed));
                update = update.content(None);
            } else if let Some(content) = content_opt {
                update = update.content(Some(content));
                update = update.embeds(None);
            }
            if update.await.is_ok() {
                let _ = HTTP.delete_all_reactions(channel_id, Id::new(msg_id)).await;
                for (_, emoji) in &info {
                    let reaction = RequestReactionType::Unicode { name: emoji };
                    let _ = HTTP
                        .create_reaction(channel_id, Id::new(msg_id), &reaction)
                        .await;
                }
                Self::set(guild_id.get(), channel_id.get(), msg_id).await;
            }
            return;
        }
        let mut create = HTTP.create_message(channel_id);
        if let Some(embed) = embed_slice {
            create = create.embeds(embed);
        } else if let Some(content) = content_opt {
            create = create.content(content);
        }

        if let Ok(response) = create.await {
            if let Ok(msg) = response.model().await {
                for (_, emoji) in &info {
                    let reaction = RequestReactionType::Unicode { name: emoji };
                    let _ = HTTP.create_reaction(channel_id, msg.id, &reaction).await;
                }
                Self::set(guild_id.get(), channel_id.get(), msg.id.get()).await;
            }
        }
    }
}
