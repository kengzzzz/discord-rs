use anyhow::Context;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::Interaction;

use crate::{
    configs::discord::{CACHE, HTTP},
    services::build::BuildService,
};

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "build", desc_localizations = "build_desc")]
pub struct WarframeBuildCommand {
    #[command(desc_localizations = "build_item_desc", autocomplete = true)]
    pub item: String,
}

fn build_item_desc() -> DescLocalizations {
    DescLocalizations::new("Item name", [("th", "ชื่อไอเทม")])
}

fn build_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Find builds from overframe.gg",
        [("th", "ค้นหา build จาก overframe.gg")],
    )
}

impl WarframeBuildCommand {
    pub async fn run(&self, interaction: Interaction) -> anyhow::Result<()> {
        let guild_id = interaction.guild_id.context("parse guild_id failed")?;
        if let Some(guild_ref) = CACHE.guild(guild_id) {
            let embeds = BuildService::build_embeds(&guild_ref, &self.item).await?;
            HTTP.interaction(interaction.application_id)
                .update_response(&interaction.token)
                .embeds(Some(&embeds))
                .await?;
        }
        Ok(())
    }
}
