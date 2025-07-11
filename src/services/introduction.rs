use std::sync::Arc;

use anyhow::Context as _;
use mongodb::bson::doc;
use twilight_model::{
    application::interaction::{Interaction, modal::ModalInteractionData},
    id::{
        Id,
        marker::{GuildMarker, UserMarker},
    },
};

use crate::{
    context::Context,
    dbs::mongo::{
        channel::{Channel, ChannelEnum},
        role::RoleEnum,
    },
    defer_interaction, guild_command, send_with_fallback,
    services::channel::ChannelService,
    utils::embed,
};

pub struct IntroductionService;

pub struct IntroDetails {
    pub name: String,
    pub age: Option<u8>,
    pub ign: Option<String>,
    pub clan: Option<String>,
}

fn value_of<'a>(data: &'a ModalInteractionData, id: &str) -> Option<&'a str> {
    for row in &data.components {
        for comp in &row.components {
            if comp.custom_id == id {
                return comp.value.as_deref();
            }
        }
    }
    None
}

fn parse_modal(data: &ModalInteractionData) -> Option<IntroDetails> {
    let name = value_of(data, "name")?.trim().to_owned();
    if name.is_empty() {
        return None;
    }

    let age = value_of(data, "age")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<u8>().ok());

    let ign = value_of(data, "ign")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned);

    let clan = value_of(data, "clan")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned);

    Some(IntroDetails {
        name,
        age,
        ign,
        clan,
    })
}

async fn handle_valid_intro(
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
        let http = ctx.http.clone();
        send_with_fallback!(http, user_id, Id::new(intro_channel.channel_id), |msg| {
            let welcome = embed::welcome_embed(&guild_ref, member_tag, &details.name)?;
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

impl IntroductionService {
    pub async fn handle_modal(
        ctx: Arc<Context>,
        interaction: Interaction,
        data: ModalInteractionData,
    ) {
        if let Err(e) = guild_command!(ctx.http, interaction, true, {
            defer_interaction!(ctx.http, &interaction, true).await?;
            let guild_id = interaction.guild_id.context("no guild id")?;
            let guild_ref = ctx.cache.guild(guild_id).context("no guild")?;
            let user = interaction.author().context("no author")?;

            let Some(intro_channel) = ChannelService::get_by_type(
                ctx.clone(),
                guild_id.get(),
                &ChannelEnum::Introduction,
            )
            .await
            else {
                if let Ok(embed) = embed::intro_unavailable_embed(&guild_ref) {
                    ctx.http
                        .interaction(interaction.application_id)
                        .update_response(&interaction.token)
                        .embeds(Some(&[embed]))
                        .await?;
                }
                return Ok(());
            };

            let Some(details) = parse_modal(&data) else {
                let embed = embed::intro_error_embed()?;
                ctx.http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[embed]))
                    .await?;
                return Ok(());
            };

            handle_valid_intro(
                ctx.clone(),
                user.id,
                guild_id,
                &intro_channel,
                &details,
                &user.name,
            )
            .await?;
            let embed = embed::intro_success_embed(&guild_ref)?;
            ctx.http
                .interaction(interaction.application_id)
                .update_response(&interaction.token)
                .embeds(Some(&[embed]))
                .await?;

            Ok::<_, anyhow::Error>(())
        })
        .await
        {
            tracing::error!(error = %e, "failed to handle intro modal");
            if let Ok(embed) = embed::intro_error_embed() {
                let _ = ctx
                    .http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[embed]))
                    .await;
            }
        }
    }
}
