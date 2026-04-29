use std::sync::Arc;

use async_trait::async_trait;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::{
    command::Command,
    interaction::{Interaction, application_command::CommandData},
};

use crate::{context::Context, features::registry::FeatureSlice, handle_ephemeral};

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "help", desc_localizations = "help_desc")]
pub struct HelpCommand;

fn help_desc() -> DescLocalizations {
    DescLocalizations::new("Show bot commands", [("th", "คำสั่งของบอท")])
}

impl HelpCommand {
    pub async fn handle(ctx: Arc<Context>, interaction: Interaction, _data: CommandData) {
        handle_ephemeral!(ctx.http, interaction, "HelpCommand", {
            let guild_id = interaction
                .guild_id
                .ok_or(anyhow::anyhow!("no guild"))?;
            if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                let help = crate::utils::embed::help_embed(&guild_ref)?;
                ctx.http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[help]))
                    .await?;
            }
        });
    }
}

pub struct HelpFeature;

#[async_trait]
impl FeatureSlice for HelpFeature {
    fn register_commands(&self, commands: &mut Vec<Command>) {
        commands.push(HelpCommand::create_command().into());
    }

    fn command_names(&self) -> &'static [&'static str] {
        &["help"]
    }

    async fn handle_command(&self, ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        HelpCommand::handle(ctx, interaction, data).await;
    }
}
