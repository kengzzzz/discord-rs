use anyhow::Context as _;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::Interaction;

use crate::{context::Context, services::build::BuildService};
use std::sync::Arc;

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
    pub async fn run(&self, ctx: Arc<Context>, interaction: Interaction) -> anyhow::Result<()> {
        let guild_id = interaction
            .guild_id
            .context("parse guild_id failed")?;
        if let Some(guild_ref) = ctx.cache.guild(guild_id) {
            let embeds = BuildService::build_embeds(&ctx.reqwest, &guild_ref, &self.item).await?;
            ctx.http
                .interaction(interaction.application_id)
                .update_response(&interaction.token)
                .embeds(Some(&embeds))
                .await?;
        }
        Ok(())
    }
}
