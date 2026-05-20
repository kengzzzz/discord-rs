use anyhow::Context as _;
use twilight_interactions::command::{
    CommandModel, CommandOption, CreateCommand, CreateOption, DescLocalizations,
};
use twilight_model::application::interaction::Interaction;

use crate::{context::Context, services::guild_settings::GuildSettingsService, utils::embed};
use std::sync::Arc;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(
    name = "scam-detect",
    desc_localizations = "admin_scam_detect_desc"
)]
pub struct AdminScamDetectCommand {
    #[command(desc_localizations = "admin_scam_detect_choice_desc")]
    pub choice: AdminScamDetectChoice,
}

#[derive(CreateOption, CommandOption, Debug, Clone, Copy)]
pub enum AdminScamDetectChoice {
    #[option(name = "Enable", value = "enable")]
    Enable,
    #[option(name = "Disable", value = "disable")]
    Disable,
}

fn admin_scam_detect_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Configure OCR scam image detection",
        [("th", "ตั้งค่าการตรวจจับรูปภาพ scam ด้วย OCR")],
    )
}

fn admin_scam_detect_choice_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Enable or disable scam image detection in this server",
        [("th", "เปิดหรือปิดการตรวจจับรูปภาพ scam ในเซิร์ฟเวอร์นี้")],
    )
}

impl AdminScamDetectCommand {
    pub async fn run(&self, ctx: Arc<Context>, interaction: Interaction) -> anyhow::Result<()> {
        let enabled = match self.choice {
            AdminScamDetectChoice::Enable => true,
            AdminScamDetectChoice::Disable => false,
        };
        let guild_id = interaction
            .guild_id
            .context("failed to parse guild_id")?;
        let author = interaction
            .author()
            .context("failed to parse author")?;

        GuildSettingsService::set_scam_detect_enabled(&ctx, guild_id.get(), enabled).await?;

        if let Some(guild_ref) = ctx.cache.guild(guild_id) {
            let embed = embed::set_scam_detect_embed(
                &guild_ref,
                enabled,
                ctx.scam_detect.enabled(),
                &author.name,
            )?;
            ctx.http
                .interaction(interaction.application_id)
                .update_response(&interaction.token)
                .embeds(Some(&[embed]))
                .await?;
        }

        Ok(())
    }
}
