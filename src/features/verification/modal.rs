use std::sync::Arc;

use anyhow::Context as _;
use twilight_model::application::interaction::{Interaction, modal::ModalInteractionData};

use crate::{
    context::Context, defer_interaction, guild_command, services::spam, services::verification,
};

pub async fn handle_verify_modal(
    ctx: Arc<Context>,
    interaction: Interaction,
    data: ModalInteractionData,
) {
    if let Err(e) = guild_command!(ctx.http, interaction, true, {
        defer_interaction!(ctx.http, &interaction, true).await?;
        let guild_id = interaction
            .guild_id
            .context("no guild id")?;
        let user = interaction
            .author()
            .context("no author")?;

        let Some(token) = verification::form::parse_modal(&data) else {
            if let Some(guild_ref) = ctx.cache.guild(guild_id)
                && let Some(embed) = verification::embed::verify_fail_embed(&guild_ref)
            {
                ctx.http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[embed]))
                    .await?;
            }
            return Ok(());
        };

        let success = spam::quarantine::verify(&ctx, guild_id, user.id, &token).await;
        if let Some(guild_ref) = ctx.cache.guild(guild_id) {
            let embed = if success {
                verification::embed::verify_success_embed(&guild_ref)
            } else {
                verification::embed::verify_fail_embed(&guild_ref)
            };
            if let Some(embed) = embed {
                ctx.http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[embed]))
                    .await?;
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    {
        tracing::error!(error = %e, "failed to handle verify modal");
    }
}
