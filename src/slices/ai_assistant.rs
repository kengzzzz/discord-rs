use std::sync::Arc;

use async_trait::async_trait;
use twilight_interactions::command::CreateCommand;
use twilight_model::application::{
    command::Command,
    interaction::{Interaction, application_command::CommandData},
};

use crate::{commands::ai::AiCommand, context::Context, slices::registry::FeatureSlice};

pub struct AiAssistantSlice;

#[async_trait]
impl FeatureSlice for AiAssistantSlice {
    fn register_commands(&self, commands: &mut Vec<Command>) {
        commands.push(AiCommand::create_command().into());
    }

    fn command_names(&self) -> &'static [&'static str] {
        &["ai"]
    }

    async fn handle_command(&self, ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        AiCommand::handle(ctx, interaction, data).await;
    }
}
