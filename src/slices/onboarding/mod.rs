pub mod api;
pub mod app;
pub mod domain;

use std::sync::Arc;

use async_trait::async_trait;
use twilight_interactions::command::CreateCommand;
use twilight_model::{
    application::{
        command::Command,
        interaction::{Interaction, application_command::CommandData, modal::ModalInteractionData},
    },
    gateway::payload::incoming::{MemberAdd, MemberRemove},
};

use crate::{
    context::Context,
    features::{intro::IntroCommand, verification::VerifyCommand},
    slices::registry::FeatureSlice,
};

pub struct OnboardingSlice;

#[async_trait]
impl FeatureSlice for OnboardingSlice {
    fn register_commands(&self, commands: &mut Vec<Command>) {
        commands.push(IntroCommand::create_command().into());
        commands.push(VerifyCommand::create_command().into());
    }

    fn command_names(&self) -> &'static [&'static str] {
        &["intro", "verify"]
    }

    fn modal_ids(&self) -> &'static [&'static str] {
        &["intro_modal", "verify_modal"]
    }

    async fn handle_command(&self, ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        match data.name.as_str() {
            "intro" => IntroCommand::handle(ctx, interaction, data).await,
            "verify" => VerifyCommand::handle(ctx, interaction, data).await,
            _ => {}
        }
    }

    async fn handle_modal(
        &self,
        ctx: Arc<Context>,
        interaction: Interaction,
        data: ModalInteractionData,
    ) {
        match data.custom_id.as_str() {
            "intro_modal" => api::modals::handle_intro_modal(ctx, interaction, data).await,
            "verify_modal" => api::modals::handle_verify_modal(ctx, interaction, data).await,
            _ => {}
        }
    }

    async fn handle_member_add(&self, ctx: Arc<Context>, event: MemberAdd) -> bool {
        api::member_events::handle_member_add(ctx, event).await;
        true
    }

    async fn handle_member_remove(&self, ctx: Arc<Context>, event: MemberRemove) -> bool {
        api::member_events::handle_member_remove(ctx, event).await;
        true
    }
}
