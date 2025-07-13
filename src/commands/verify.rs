use anyhow::Context as _;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::{Interaction, application_command::CommandData};

use crate::{context::Context, handle_ephemeral, services::spam};
use std::sync::Arc;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "verify", desc_localizations = "verify_desc")]
pub struct VerifyCommand {
    #[command(desc_localizations = "verify_token_desc")]
    pub token: String,
}

fn verify_desc() -> DescLocalizations {
    DescLocalizations::new("Confirm you are not a bot", [("th", "ยืนยันตัวตน")])
}

fn verify_token_desc() -> DescLocalizations {
    DescLocalizations::new("Verification token", [("th", "โทเคนยืนยัน")])
}

impl VerifyCommand {
    pub async fn handle(ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        handle_ephemeral!(ctx.http, interaction, "VerifyCommand", {
            let command = VerifyCommand::from_interaction(data.into())
                .context("failed to parse command data")?;

            let author = interaction.author().context("failed to get author")?;
            let guild_id = interaction.guild_id.context("no guild id")?;
            let success =
                spam::quarantine::verify(ctx.clone(), guild_id, author.id, &command.token).await;
            let embed = if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                if success {
                    embed::verify_success_embed(&guild_ref)
                } else {
                    embed::verify_fail_embed(&guild_ref)
                }
            } else {
                None
            };

            if let Some(embed) = embed {
                ctx.http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[embed]))
                    .await?;
            }
        });
    }
}
