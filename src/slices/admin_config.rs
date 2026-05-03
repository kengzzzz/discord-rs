use std::sync::Arc;

use async_trait::async_trait;
use twilight_interactions::command::CreateCommand;
use twilight_model::{
    application::{
        command::Command,
        interaction::{Interaction, application_command::CommandData},
    },
    guild::Permissions,
};

use crate::{commands::admin::AdminCommand, context::Context, slices::registry::FeatureSlice};

pub struct AdminConfigSlice;

#[async_trait]
impl FeatureSlice for AdminConfigSlice {
    fn register_commands(&self, commands: &mut Vec<Command>) {
        let mut command = AdminCommand::create_command();
        command.default_member_permissions = Some(Permissions::ADMINISTRATOR);
        commands.push(command.into());
    }

    fn command_names(&self) -> &'static [&'static str] {
        &["admin"]
    }

    fn autocomplete_names(&self) -> &'static [&'static str] {
        &["admin"]
    }

    async fn handle_command(&self, ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        AdminCommand::handle(ctx, interaction, data).await;
    }

    async fn handle_autocomplete(
        &self,
        ctx: Arc<Context>,
        interaction: Interaction,
        data: CommandData,
    ) {
        AdminCommand::autocomplete(ctx, interaction, data).await;
    }
}
