use anyhow::Context as _;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::{Interaction, application_command::CommandData};
use twilight_model::channel::message::component::{
    ActionRow, Component, TextInput, TextInputStyle,
};

use crate::guild_command;
use crate::services::verification;
use crate::{context::Context, services::spam};
use std::sync::Arc;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "verify", desc_localizations = "verify_desc")]
pub struct VerifyCommand;

fn verify_desc() -> DescLocalizations {
    DescLocalizations::new("Confirm you are not a bot", [("th", "ยืนยันตัวตน")])
}

impl VerifyCommand {
    pub async fn handle(ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        if let Err(e) = guild_command!(ctx.http, interaction, true, {
            let _command = VerifyCommand::from_interaction(data.into())
                .context("failed to parse command data")?;

            let author = interaction
                .author()
                .context("failed to get author")?;
            let guild_id = interaction
                .guild_id
                .context("no guild id")?;

            let Some(token) =
                spam::quarantine::get_token(&ctx, guild_id.get(), author.id.get()).await
            else {
                if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                    if let Some(embed) = verification::embed::verify_no_token_embed(&guild_ref) {
                        let response_data = InteractionResponseData {
                            embeds: Some(vec![embed]),
                            flags: Some(MessageFlags::EPHEMERAL),
                            ..Default::default()
                        };
                        let response = InteractionResponse {
                            kind: InteractionResponseType::ChannelMessageWithSource,
                            data: Some(response_data),
                        };
                        ctx.http
                            .interaction(interaction.application_id)
                            .create_response(interaction.id, &interaction.token, &response)
                            .await?;
                    }
                }
                return Ok(());
            };

            let components = vec![Component::ActionRow(ActionRow {
                components: vec![Component::TextInput(TextInput {
                    custom_id: "token".into(),
                    label: "Token".into(),
                    max_length: None,
                    min_length: Some(1),
                    placeholder: None,
                    required: Some(true),
                    style: TextInputStyle::Short,
                    value: Some(token),
                })],
            })];

            let data = InteractionResponseData {
                allowed_mentions: None,
                attachments: None,
                choices: None,
                components: Some(components),
                content: None,
                custom_id: Some("verify_modal".into()),
                embeds: None,
                flags: None,
                title: Some("Verify".into()),
                tts: None,
            };

            let response =
                InteractionResponse { kind: InteractionResponseType::Modal, data: Some(data) };

            ctx.http
                .interaction(interaction.application_id)
                .create_response(interaction.id, &interaction.token, &response)
                .await?;
            Ok(())
        })
        .await
        {
            tracing::error!(error = %e, "error handling VerifyCommand");
        }
    }
}
