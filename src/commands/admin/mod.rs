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
    commands::admin::{channel::AdminChannelCommand, role::AdminRoleCommand},
    context::Context,
    handle_ephemeral,
    utils::ascii::ascii_starts_with_icase,
};
use std::sync::Arc;

pub mod channel;
pub mod role;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "admin", desc_localizations = "admin_desc")]
pub enum AdminCommand {
    #[command(name = "channel")]
    Channel(AdminChannelCommand),
    #[command(name = "role")]
    Role(AdminRoleCommand),
}

fn admin_desc() -> DescLocalizations {
    DescLocalizations::new("Bot setting for admin", [("th", "การตั้งค่าบอทสำหรับ admin")])
}

fn extract_focused(cmd: &CommandData) -> Option<(&str, &str)> {
    for opt in &cmd.options {
        match &opt.value {
            CommandOptionValue::SubCommand(sub_opts) => {
                for nested in sub_opts {
                    if let CommandOptionValue::Focused(user_input, _) = &nested.value {
                        return Some((nested.name.as_str(), user_input.as_str()));
                    }
                }
            }
            CommandOptionValue::SubCommandGroup(group_opts) => {
                for subc in group_opts {
                    if let CommandOptionValue::SubCommand(sub_opts) = &subc.value {
                        for nested in sub_opts {
                            if let CommandOptionValue::Focused(user_input, _) = &nested.value {
                                return Some((nested.name.as_str(), user_input.as_str()));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

impl AdminCommand {
    pub async fn handle(ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        handle_ephemeral!(ctx.http, interaction, "AdminCommand", {
            let command = AdminCommand::from_interaction(data.into())
                .context("failed to parse command data")?;

            match command {
                AdminCommand::Channel(command) => command.run(ctx.clone(), interaction).await,
                AdminCommand::Role(command) => command.run(ctx.clone(), interaction).await,
            }?;
        });
    }

    pub async fn autocomplete(ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        if let Err(e) = async {
            let focused = extract_focused(&data).context("parse focused field failed")?;
            let guild_id = interaction.guild_id.context("parse guild_id failed")?;

            let mut choices = Vec::with_capacity(25);

            let prefix = focused.1.to_ascii_lowercase();

            if focused.0 == "role_name" {
                if let Some(role_ids) = ctx.cache.guild_roles(guild_id) {
                    choices.extend(
                        role_ids
                            .iter()
                            .filter_map(|role_id| {
                                ctx.cache.role(*role_id).and_then(|role| {
                                    if ascii_starts_with_icase(&role.name, &prefix) {
                                        Some(CommandOptionChoice {
                                            name: role.name.clone(),
                                            value: CommandOptionChoiceValue::String(
                                                role.id.to_string(),
                                            ),
                                            name_localizations: None,
                                        })
                                    } else {
                                        None
                                    }
                                })
                            })
                            .take(25),
                    );
                }
            }

            let response = InteractionResponse {
                kind: InteractionResponseType::ApplicationCommandAutocompleteResult,
                data: Some(InteractionResponseData {
                    choices: Some(choices),
                    ..InteractionResponseData::default()
                }),
            };

            ctx.http
                .interaction(interaction.application_id)
                .create_response(interaction.id, &interaction.token, &response)
                .await?;

            Ok::<_, anyhow::Error>(())
        }
        .await
        {
            tracing::error!(error = %e, "autocomplete handler failed");
        }
    }
}
