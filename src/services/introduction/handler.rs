use std::sync::Arc;

use mongodb::bson::doc;
use twilight_model::id::{
    Id,
    marker::{GuildMarker, UserMarker},
};

use crate::{
    context::Context,
    dbs::mongo::models::{channel::Channel, role::RoleEnum},
    send_with_fallback,
    utils::embed::welcome_embed,
};

use super::embed;
use super::form::IntroDetails;

pub async fn handle_valid_intro(
    ctx: Arc<Context>,
    user_id: Id<UserMarker>,
    guild_id: Id<GuildMarker>,
    intro_channel: &Channel,
    details: &IntroDetails,
    member_tag: &str,
) -> anyhow::Result<()> {
    let db = ctx.mongo.clone();

    let roles = tokio::try_join!(
        db.roles.find_one(doc! {
            "role_type": RoleEnum::Guest.value(),
            "guild_id": guild_id.get() as i64,
        }),
        db.roles.find_one(doc! {
            "role_type": RoleEnum::Member.value(),
            "guild_id": guild_id.get() as i64,
        }),
    )?;

    if let Some(role_to_remove) = roles.0 {
        ctx.http
            .remove_guild_member_role(guild_id, user_id, Id::new(role_to_remove.role_id))
            .await?;
    }
    if let Some(role_to_add) = roles.1 {
        ctx.http
            .add_guild_member_role(guild_id, user_id, Id::new(role_to_add.role_id))
            .await?;
    }

    if let Some(guild_ref) = ctx.cache.guild(guild_id) {
        let intro_embed = embed::intro_details_embed(&guild_ref, member_tag, details)?;
        send_with_fallback!(ctx, user_id, Id::new(intro_channel.channel_id), |msg| {
            let welcome = welcome_embed(&guild_ref, member_tag, &details.name)?;
            msg.embeds(&[welcome]).await?;
            Ok::<_, anyhow::Error>(())
        });
        ctx.http
            .create_message(Id::new(intro_channel.channel_id))
            .embeds(&[intro_embed])
            .await?;
    }

    Ok(())
}
