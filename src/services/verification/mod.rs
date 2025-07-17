use std::sync::Arc;

use anyhow::Context as _;
use twilight_model::application::interaction::{Interaction, modal::ModalInteractionData};

use crate::{context::Context, defer_interaction, guild_command, services::spam};

pub mod embed;
pub(crate) mod form;

pub struct VerificationService;

impl VerificationService {
    pub async fn handle_modal(
        ctx: Arc<Context>,
        interaction: Interaction,
        data: ModalInteractionData,
    ) {
        if let Err(e) = guild_command!(ctx.http, interaction, true, {
            defer_interaction!(ctx.http, &interaction, true).await?;
            let guild_id = interaction.guild_id.context("no guild id")?;
            let user = interaction.author().context("no author")?;

            let Some(token) = form::parse_modal(&data) else {
                if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                    if let Some(embed) = self::embed::verify_fail_embed(&guild_ref) {
                        ctx.http
                            .interaction(interaction.application_id)
                            .update_response(&interaction.token)
                            .embeds(Some(&[embed]))
                            .await?;
                    }
                }
                return Ok(());
            };

            let success = spam::quarantine::verify(ctx.clone(), guild_id, user.id, &token).await;
            if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                let embed = if success {
                    self::embed::verify_success_embed(&guild_ref)
                } else {
                    self::embed::verify_fail_embed(&guild_ref)
                };
                if let Some(embed) = embed {
                    ctx.http
                        .interaction(interaction.application_id)
                        .update_response(&interaction.token)
                        .embeds(Some(&[embed]))
                        .await?;
                }
            }
            Ok(())
        })
        .await
        {
            tracing::error!(error = %e, "failed to handle verify modal");
        }
    }
}

#[cfg(test)]
mod tests;
