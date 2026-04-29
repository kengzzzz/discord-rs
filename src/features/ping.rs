use std::sync::Arc;

use async_trait::async_trait;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::{
    command::Command,
    interaction::{Interaction, application_command::CommandData},
};

use crate::{
    context::Context, features::registry::FeatureSlice, handle_ephemeral,
    services::latency::LatencyService,
};

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "ping", desc_localizations = "ping_desc")]
pub struct PingCommand;

fn ping_desc() -> DescLocalizations {
    DescLocalizations::new("Show bot latency", [("th", "ดูความหน่วงของบอท")])
}

impl PingCommand {
    pub async fn handle(ctx: Arc<Context>, interaction: Interaction, _data: CommandData) {
        handle_ephemeral!(ctx.http, interaction, "PingCommand", {
            let latency = LatencyService::get();
            let pong = crate::utils::embed::pong_embed(latency)?;
            ctx.http
                .interaction(interaction.application_id)
                .update_response(&interaction.token)
                .embeds(Some(&[pong]))
                .await?;
        });
    }
}

pub struct PingFeature;

#[async_trait]
impl FeatureSlice for PingFeature {
    fn register_commands(&self, commands: &mut Vec<Command>) {
        commands.push(PingCommand::create_command().into());
    }

    fn command_names(&self) -> &'static [&'static str] {
        &["ping"]
    }

    async fn handle_command(&self, ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        PingCommand::handle(ctx, interaction, data).await;
    }
}
