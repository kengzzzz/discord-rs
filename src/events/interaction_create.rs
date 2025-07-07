use std::mem;
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};

use crate::{
    commands::{
        admin::AdminCommand, ai::AiCommand, help::HelpCommand, intro::IntroCommand,
        ping::PingCommand, verify::VerifyCommand, warframe::WarframeCommand,
    },
    services::{introduction::IntroductionService, market::MarketService},
};

pub async fn handle(interaction: Interaction) {
    let Some(user) = &interaction.author() else {
        return;
    };
    if user.bot | user.system.unwrap_or_default() {
        return;
    }

    let mut interaction = interaction;

    let data = match mem::take(&mut interaction.data) {
        Some(InteractionData::ApplicationCommand(data)) => {
            if interaction.kind == InteractionType::ApplicationCommandAutocomplete {
                match &*data.name {
                    "admin" => AdminCommand::autocomplete(interaction, *data).await,
                    "warframe" => WarframeCommand::autocomplete(interaction, *data).await,
                    _ => {}
                }
                return;
            }
            *data
        }
        Some(InteractionData::MessageComponent(data)) => {
            MarketService::handle_component(interaction, *data).await;
            return;
        }
        Some(InteractionData::ModalSubmit(data)) => {
            IntroductionService::handle_modal(interaction, data).await;
            return;
        }
        _ => {
            tracing::warn!("ignoring non-command interaction");
            return;
        }
    };

    match &*data.name {
        "admin" => {
            AdminCommand::handle(interaction, data).await;
        }
        "verify" => {
            VerifyCommand::handle(interaction, data).await;
        }
        "warframe" => {
            WarframeCommand::handle(interaction, data).await;
        }
        "ai" => {
            AiCommand::handle(interaction, data).await;
        }
        "ping" => {
            PingCommand::handle(interaction, data).await;
        }
        "intro" => {
            IntroCommand::handle(interaction, data).await;
        }
        "help" => {
            HelpCommand::handle(interaction, data).await;
        }
        _ => {}
    }
}
