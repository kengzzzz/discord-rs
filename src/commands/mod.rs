pub mod admin;
pub mod ai;
pub mod general;
pub mod warframe;

use std::sync::Arc;

use twilight_interactions::command::{CommandInputData, CommandModel, CreateCommand};
use twilight_model::{
    application::{
        command::{Command, CommandType},
        interaction::{
            Interaction, InteractionContextType,
            application_command::{CommandData, CommandOptionValue},
        },
    },
    oauth::ApplicationIntegrationType,
};
use twilight_util::builder::command::CommandBuilder;

use crate::{app::App, features::onboarding};

pub const TRANSLATE_COMMAND: &str = "Translate to Thai";

pub fn all() -> Vec<Command> {
    vec![
        admin::AdminCommand::create_command().into(),
        admin::ModCommand::create_command().into(),
        ai::AiCommand::create_command().into(),
        warframe::WarframeCommand::create_command().into(),
        general::HelpCommand::create_command().into(),
        general::PingCommand::create_command().into(),
        general::IntroCommand::create_command().into(),
        general::VerifyCommand::create_command().into(),
        translate_context_menu(),
    ]
}

fn translate_context_menu() -> Command {
    CommandBuilder::new(TRANSLATE_COMMAND, "", CommandType::Message)
        .name_localizations([("th", "แปลเป็นภาษาไทย")])
        .contexts([InteractionContextType::Guild])
        .integration_types([ApplicationIntegrationType::GuildInstall])
        .build()
}

pub fn parse<T: CommandModel>(data: &CommandData) -> anyhow::Result<T> {
    T::from_interaction(CommandInputData::from(data.clone()))
        .map_err(|e| anyhow::anyhow!("invalid command data: {e:?}"))
}

pub async fn dispatch(
    app: &Arc<App>,
    interaction: &Interaction,
    data: &CommandData,
) -> anyhow::Result<()> {
    match data.name.as_str() {
        "admin" => admin::run_admin(app, interaction, parse(data)?).await,
        "mod" => admin::run_mod(app, interaction, parse(data)?).await,
        "ai" => ai::run(app, interaction, parse(data)?).await,
        "warframe" => warframe::run(app, interaction, parse(data)?).await,
        "help" => general::help(app, interaction).await,
        "ping" => general::ping(app, interaction).await,
        "intro" => onboarding::open_intro(app, interaction).await,
        "verify" => onboarding::open_verify(app, interaction).await,
        _ => Ok(()),
    }
}

pub fn focused(data: &CommandData) -> Option<(&str, &str, &str)> {
    for option in &data.options {
        if let CommandOptionValue::SubCommand(nested) = &option.value {
            for inner in nested {
                if let CommandOptionValue::Focused(value, _) = &inner.value {
                    return Some((
                        option.name.as_str(),
                        inner.name.as_str(),
                        value.as_str(),
                    ));
                }
            }
        }
        if let CommandOptionValue::Focused(value, _) = &option.value {
            return Some(("", option.name.as_str(), value.as_str()));
        }
    }
    None
}

pub async fn autocomplete(
    app: &Arc<App>,
    interaction: &Interaction,
    data: &CommandData,
) -> anyhow::Result<()> {
    let choices = match (data.name.as_str(), focused(data)) {
        ("warframe", Some(("market", "item", value))) => app.warframe.market.complete(value),
        ("warframe", Some(("build", "item", value))) => app.warframe.builds.complete(value),
        _ => Vec::new(),
    };
    crate::discord::reply::autocomplete(app, interaction, choices).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_passes_discord_validation() {
        let commands = all();
        assert_eq!(commands.len(), 9);
        for command in &commands {
            twilight_validate::command::command(command)
                .unwrap_or_else(|e| panic!("{}: {e}", command.name));
            assert_eq!(
                command.integration_types.as_deref(),
                Some([ApplicationIntegrationType::GuildInstall].as_slice()),
                "{}",
                command.name
            );
        }
    }
}
