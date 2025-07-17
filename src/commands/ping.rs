use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::{Interaction, application_command::CommandData};

use crate::context::Context;
use crate::handle_ephemeral;
use crate::services::latency::LatencyService;
use std::sync::Arc;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "ping", desc_localizations = "ping_desc")]
pub struct PingCommand {}

fn ping_desc() -> DescLocalizations {
    DescLocalizations::new("Show bot latency", [("th", "ดูความหน่วงของบอท")])
}

impl PingCommand {
    pub async fn handle(ctx: Arc<Context>, interaction: Interaction, _data: CommandData) {
        handle_ephemeral!(ctx.http, interaction, "PingCommand", {
            let latency = LatencyService::get();
            let embed = embed::pong_embed(latency)?;
            ctx.http
                .interaction(interaction.application_id)
                .update_response(&interaction.token)
                .embeds(Some(&[embed]))
                .await?;
        });
    }
}
