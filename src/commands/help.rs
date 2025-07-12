use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::{Interaction, application_command::CommandData};

use crate::{context::Context, handle_ephemeral};
use std::sync::Arc;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "help", desc_localizations = "help_desc")]
pub struct HelpCommand {}

fn help_desc() -> DescLocalizations {
    DescLocalizations::new("Show bot commands", [("th", "คำสั่งของบอท")])
}

impl HelpCommand {
    pub async fn handle(ctx: Arc<Context>, interaction: Interaction, _data: CommandData) {
        handle_ephemeral!(ctx.http, interaction, "HelpCommand", {
            let guild_id = interaction.guild_id.ok_or(anyhow::anyhow!("no guild"))?;
            if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                let embed = embed::help_embed(&guild_ref)?;
                ctx.http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[embed]))
                    .await?;
            }
        });
    }
}
