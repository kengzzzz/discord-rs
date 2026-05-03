use std::sync::Arc;

use async_trait::async_trait;
use twilight_interactions::command::CreateCommand;
use twilight_model::{
    application::{
        command::Command,
        interaction::{Interaction, application_command::CommandData},
    },
    gateway::payload::incoming::{Ready, VoiceStateUpdate},
};

use crate::{
    context::Context,
    events::{ready, voice_state_update},
    features::{help::HelpCommand, ping::PingCommand},
    slices::registry::FeatureSlice,
};

pub struct RuntimeSlice;

#[async_trait]
impl FeatureSlice for RuntimeSlice {
    fn register_commands(&self, commands: &mut Vec<Command>) {
        commands.push(PingCommand::create_command().into());
        commands.push(HelpCommand::create_command().into());
    }

    fn command_names(&self) -> &'static [&'static str] {
        &["ping", "help"]
    }

    async fn handle_command(&self, ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        match data.name.as_str() {
            "ping" => PingCommand::handle(ctx, interaction, data).await,
            "help" => HelpCommand::handle(ctx, interaction, data).await,
            _ => {}
        }
    }

    async fn handle_ready(&self, ctx: Arc<Context>, event: Ready) -> bool {
        ready::handle(ctx, event).await;
        true
    }

    async fn handle_voice_state_update(&self, ctx: Arc<Context>, event: VoiceStateUpdate) -> bool {
        voice_state_update::handle(ctx, event).await;
        true
    }
}
