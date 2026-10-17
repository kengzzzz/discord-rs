use std::sync::Arc;

use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::Interaction;

use crate::{context::Context, warframe::circuit};

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "circuit", desc_localizations = "circuit_desc")]
pub struct WarframeCircuitCommand;

fn circuit_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Check this week's Circuit and upcoming Incarnons",
        [("th", "ดู Circuit สัปดาห์นี้และอาวุธ Incarnon สัปดาห์ถัดไป")],
    )
}

impl WarframeCircuitCommand {
    pub async fn run(&self, ctx: Arc<Context>, interaction: Interaction) -> anyhow::Result<()> {
        let embed = match circuit::circuit_data(&ctx)
            .await
            .and_then(|data| circuit::schedule_embed(&data))
        {
            Ok(embed) => embed,
            Err(error) => {
                tracing::warn!(error = %error, "failed to fetch Circuit rotation");
                circuit::unavailable_embed()
            }
        };
        ctx.http
            .interaction(interaction.application_id)
            .update_response(&interaction.token)
            .embeds(Some(&[embed]))
            .await?;
        Ok(())
    }
}
