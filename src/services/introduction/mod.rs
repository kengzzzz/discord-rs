use std::sync::Arc;

use anyhow::Context as _;
use twilight_model::application::interaction::{Interaction, modal::ModalInteractionData};

use crate::{
    context::Context, dbs::mongo::models::channel::ChannelEnum, defer_interaction, guild_command,
    services::channel::ChannelService,
};

pub mod embed;
pub mod form;
pub mod handler;

pub struct IntroductionService;

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

            let Some(intro_channel) =
                ChannelService::get_by_type(&ctx, guild_id.get(), &ChannelEnum::Introduction).await
            else {
                if let Ok(embed) = self::embed::intro_unavailable_embed(&guild_ref) {
                    ctx.http
                        .interaction(interaction.application_id)
                        .update_response(&interaction.token)
                        .embeds(Some(&[embed]))
                        .await?;
                }
                return Ok(());
            };

            let Some(details) = form::parse_modal(&data) else {
                let embed = self::embed::intro_error_embed()?;
                ctx.http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[embed]))
                    .await?;
                return Ok(());
            };

            handler::handle_valid_intro(
                &ctx,
                user.id,
                guild_id,
                &intro_channel,
                &details,
                &user.name,
            )
            .await?;
            let embed = self::embed::intro_success_embed(&guild_ref)?;
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
                if let Err(e2) = ctx
                    .http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[embed]))
                    .await
                {
                    tracing::warn!(error = %e2, "failed to send intro error response");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
