use std::sync::Arc;

use async_trait::async_trait;
use twilight_interactions::command::CreateCommand;
use twilight_model::application::{
    command::Command,
    interaction::{Interaction, application_command::CommandData},
};

use crate::{
    commands::warframe::WarframeCommand, context::Context, features::registry::FeatureSlice,
};

pub struct WarframeFeature;

#[async_trait]
impl FeatureSlice for WarframeFeature {
    fn register_commands(&self, commands: &mut Vec<Command>) {
        commands.push(WarframeCommand::create_command().into());
    }

    fn command_names(&self) -> &'static [&'static str] {
        &["warframe"]
    }

    fn autocomplete_names(&self) -> &'static [&'static str] {
        &["warframe"]
    }

    async fn handle_command(&self, ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        WarframeCommand::handle(ctx, interaction, data).await;
    }

    async fn handle_autocomplete(
        &self,
        ctx: Arc<Context>,
        interaction: Interaction,
        data: CommandData,
    ) {
        WarframeCommand::autocomplete(ctx, interaction, data).await;
    }
}
