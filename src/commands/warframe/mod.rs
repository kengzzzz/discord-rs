use anyhow::Context as _;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::{
    application::{
        command::{CommandOptionChoice, CommandOptionChoiceValue},
        interaction::{
            Interaction,
            application_command::{CommandData, CommandOptionValue},
        },
    },
    http::interaction::{InteractionResponse, InteractionResponseData, InteractionResponseType},
};

use crate::{
    context::Context,
    handle_ephemeral,
    services::{build::BuildService, market::MarketService},
};
use std::sync::Arc;

mod build;
mod market;
use build::WarframeBuildCommand;
use market::WarframeMarketCommand;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "warframe", desc_localizations = "warframe_desc")]
pub enum WarframeCommand {
    #[command(name = "build")]
    Build(WarframeBuildCommand),
    #[command(name = "market")]
    Market(WarframeMarketCommand),
}

fn warframe_desc() -> DescLocalizations {
    DescLocalizations::new("Warframe utilities", [("th", "ตัวช่วย Warframe")])
}

fn extract_focused(cmd: &CommandData) -> Option<(&str, &str, &str)> {
    for opt in &cmd.options {
        if let CommandOptionValue::SubCommand(sub_opts) = &opt.value {
            for nested in sub_opts {
                if let CommandOptionValue::Focused(user_input, _) = &nested.value {
                    return Some((opt.name.as_str(), nested.name.as_str(), user_input.as_str()));
                }
            }
        }
    }
    None
}

impl WarframeCommand {
    pub async fn handle(ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        handle_ephemeral!(ctx.http, interaction, "WarframeCommand", {
            let command = WarframeCommand::from_interaction(data.into())
                .context("failed to parse command data")?;
            match command {
                WarframeCommand::Build(cmd) => cmd.run(ctx.clone(), interaction).await?,
                WarframeCommand::Market(cmd) => cmd.run(ctx.clone(), interaction).await?,
            }
        });
    }

    pub async fn autocomplete(ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        if let Some((sub, name, user_input)) = extract_focused(&data) {
            let mut choices = Vec::new();
            if name == "item" {
                let results = if sub == "build" {
                    BuildService::search_with_update(ctx.reqwest.as_ref(), user_input).await
                } else {
                    MarketService::search_with_update(ctx.clone(), user_input).await
                };
                choices.extend(results.into_iter().map(|item| CommandOptionChoice {
                    name: item.clone(),
                    value: CommandOptionChoiceValue::String(item),
                    name_localizations: None,
                }));
            }

            let response = InteractionResponse {
                kind: InteractionResponseType::ApplicationCommandAutocompleteResult,
                data: Some(InteractionResponseData {
                    choices: Some(choices),
                    ..InteractionResponseData::default()
                }),
            };

            if let Err(e) = ctx
                .http
                .interaction(interaction.application_id)
                .create_response(interaction.id, &interaction.token, &response)
                .await
            {
                tracing::error!(error = %e, "autocomplete handler failed");
            }
        }
    }
}
