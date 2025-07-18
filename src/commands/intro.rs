use anyhow::Context as _;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::{Interaction, application_command::CommandData};
use twilight_model::channel::message::component::{
    ActionRow, Component, TextInput, TextInputStyle,
};

use crate::guild_command;
use crate::services::introduction;
use crate::{
    context::Context, dbs::mongo::models::channel::ChannelEnum, services::channel::ChannelService,
};
use std::sync::Arc;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "intro", desc_localizations = "intro_desc")]
pub struct IntroCommand {}

fn intro_desc() -> DescLocalizations {
    DescLocalizations::new("Introduce yourself", [("th", "แนะนำตัวเอง")])
}

impl IntroCommand {
    pub async fn handle(ctx: Arc<Context>, interaction: Interaction, _data: CommandData) {
        if let Err(e) = guild_command!(ctx.http, interaction, true, {
            let guild_id = interaction.guild_id.context("no guild id")?;
            let guild_ref = ctx.cache.guild(guild_id).context("no guild")?;
            if ChannelService::get_by_type(&ctx, guild_id.get(), &ChannelEnum::Introduction)
                .await
                .is_none()
            {
                let embed = introduction::embed::intro_unavailable_embed(&guild_ref)?;
                let data = InteractionResponseData {
                    allowed_mentions: None,
                    attachments: None,
                    choices: None,
                    components: None,
                    content: None,
                    custom_id: None,
                    embeds: Some(vec![embed]),
                    flags: Some(twilight_model::channel::message::MessageFlags::EPHEMERAL),
                    title: None,
                    tts: None,
                };
                if let Err(e) = ctx
                    .http
                    .interaction(interaction.application_id)
                    .create_response(
                        interaction.id,
                        &interaction.token,
                        &InteractionResponse {
                            kind: InteractionResponseType::ChannelMessageWithSource,
                            data: Some(data),
                        },
                    )
                    .await
                {
                    tracing::warn!(error = %e, "failed to send intro unavailable response");
                }
                return Ok::<_, anyhow::Error>(());
            }

            let components = vec![
                Component::ActionRow(ActionRow {
                    components: vec![Component::TextInput(TextInput {
                        custom_id: "name".into(),
                        label: "Name".into(),
                        max_length: None,
                        min_length: Some(1),
                        placeholder: Some("ชื่อของคุณ".into()),
                        required: Some(true),
                        style: TextInputStyle::Short,
                        value: None,
                    })],
                }),
                Component::ActionRow(ActionRow {
                    components: vec![Component::TextInput(TextInput {
                        custom_id: "age".into(),
                        label: "Age".into(),
                        max_length: None,
                        min_length: None,
                        placeholder: Some("อายุของคุณ(ไม่บังคับ)".into()),
                        required: Some(false),
                        style: TextInputStyle::Short,
                        value: None,
                    })],
                }),
                Component::ActionRow(ActionRow {
                    components: vec![Component::TextInput(TextInput {
                        custom_id: "ign".into(),
                        label: "IGN".into(),
                        max_length: None,
                        min_length: None,
                        placeholder: Some("ชื่อในเกม(ไม่บังคับ)".into()),
                        required: Some(false),
                        style: TextInputStyle::Short,
                        value: None,
                    })],
                }),
                Component::ActionRow(ActionRow {
                    components: vec![Component::TextInput(TextInput {
                        custom_id: "clan".into(),
                        label: "Clan".into(),
                        max_length: None,
                        min_length: None,
                        placeholder: Some("ชื่อแคลน(ไม่บังคับ)".into()),
                        required: Some(false),
                        style: TextInputStyle::Short,
                        value: None,
                    })],
                }),
            ];

            let data = InteractionResponseData {
                allowed_mentions: None,
                attachments: None,
                choices: None,
                components: Some(components),
                content: None,
                custom_id: Some("intro_modal".into()),
                embeds: None,
                flags: None,
                title: Some("Introduce Yourself".into()),
                tts: None,
            };

            let response = InteractionResponse {
                kind: InteractionResponseType::Modal,
                data: Some(data),
            };

            if let Err(e) = ctx
                .http
                .interaction(interaction.application_id)
                .create_response(interaction.id, &interaction.token, &response)
                .await
            {
                tracing::error!(error = %e, "failed to create intro modal");
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        {
            tracing::error!(error = %e, "error handling IntroCommand");
        }
    }
}
