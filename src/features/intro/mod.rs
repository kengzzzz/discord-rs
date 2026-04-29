pub mod command;
pub mod modal;

use std::sync::Arc;

use async_trait::async_trait;
use twilight_interactions::command::CreateCommand;
use twilight_model::application::{
    command::Command,
    interaction::{Interaction, application_command::CommandData, modal::ModalInteractionData},
};

use crate::{context::Context, features::registry::FeatureSlice};

pub use command::IntroCommand;

pub struct IntroFeature;

#[async_trait]
impl FeatureSlice for IntroFeature {
    fn register_commands(&self, commands: &mut Vec<Command>) {
        commands.push(IntroCommand::create_command().into());
    }

    fn command_names(&self) -> &'static [&'static str] {
        &["intro"]
    }

    fn modal_ids(&self) -> &'static [&'static str] {
        &["intro_modal"]
    }

    async fn handle_command(&self, ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        IntroCommand::handle(ctx, interaction, data).await;
    }

    async fn handle_modal(
        &self,
        ctx: Arc<Context>,
        interaction: Interaction,
        data: ModalInteractionData,
    ) {
        modal::handle_intro_modal(ctx, interaction, data).await;
    }
}
