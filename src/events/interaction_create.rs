use std::mem;
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};

use crate::{
    commands::{
        admin::AdminCommand, ai::AiCommand, help::HelpCommand, intro::IntroCommand,
        ping::PingCommand, verify::VerifyCommand, warframe::WarframeCommand,
    },
    context::Context,
    services::{introduction::IntroductionService, market::MarketService},
};
use std::sync::Arc;

pub async fn handle(ctx: Arc<Context>, interaction: Interaction) {
    let Some(user) = &interaction.author() else {
        return;
    };
    if user.bot || user.system.unwrap_or_default() {
        return;
    }

    let mut interaction = interaction;

    let data = match mem::take(&mut interaction.data) {
        Some(InteractionData::ApplicationCommand(data)) => {
            if interaction.kind == InteractionType::ApplicationCommandAutocomplete {
                match &*data.name {
                    "admin" => AdminCommand::autocomplete(ctx.clone(), interaction, *data).await,
                    "warframe" => {
                        WarframeCommand::autocomplete(ctx.clone(), interaction, *data).await
                    }
                    _ => {}
                }
                return;
            }
            *data
        }
        Some(InteractionData::MessageComponent(data)) => {
            MarketService::handle_component(ctx.clone(), interaction, *data).await;
            return;
        }
        Some(InteractionData::ModalSubmit(data)) => {
            IntroductionService::handle_modal(ctx.clone(), interaction, data).await;
            return;
        }
        _ => {
            tracing::warn!("ignoring non-command interaction");
            return;
        }
    };

    match &*data.name {
        "admin" => {
            AdminCommand::handle(ctx.clone(), interaction, data).await;
        }
        "verify" => {
            VerifyCommand::handle(ctx.clone(), interaction, data).await;
        }
        "warframe" => {
            WarframeCommand::handle(ctx.clone(), interaction, data).await;
        }
        "ai" => {
            AiCommand::handle(ctx.clone(), interaction, data).await;
        }
        "ping" => {
            PingCommand::handle(ctx.clone(), interaction, data).await;
        }
        "intro" => {
            IntroCommand::handle(ctx.clone(), interaction, data).await;
        }
        "help" => {
            HelpCommand::handle(ctx, interaction, data).await;
        }
        _ => {}
    }
}
