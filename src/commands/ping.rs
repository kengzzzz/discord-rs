use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::{Interaction, application_command::CommandData};

use crate::handle_ephemeral;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "ping", desc_localizations = "ping_desc")]
pub struct PingCommand {}

fn ping_desc() -> DescLocalizations {
    DescLocalizations::new("Show bot latency", [("th", "ดูความหน่วงของบอท")])
}

impl PingCommand {
    pub async fn handle(interaction: Interaction, _data: CommandData) {
        handle_ephemeral!(interaction, "PingCommand", {
            let start = std::time::Instant::now();
            let embed = embed::pinging_embed()?;
            HTTP.interaction(interaction.application_id)
                .update_response(&interaction.token)
                .embeds(Some(&[embed]))
                .await?;
            let latency = start.elapsed().as_millis();
            let embed = embed::pong_embed(latency)?;
            HTTP.interaction(interaction.application_id)
                .update_response(&interaction.token)
                .embeds(Some(&[embed]))
                .await?;
        });
    }
}
