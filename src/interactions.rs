use std::sync::Arc;

use twilight_model::application::{
    command::CommandType,
    interaction::{Interaction, InteractionData, InteractionType},
};

use crate::{
    ai,
    app::App,
    commands,
    discord::{errors, reply, ui},
    features::{admin_panel, onboarding, roles},
    moderation::quarantine,
    warframe::{market, status},
};

pub async fn handle(app: Arc<App>, interaction: Interaction) {
    if interaction
        .author()
        .is_some_and(|u| u.bot)
    {
        return;
    }
    if user_installed(&interaction) {
        metrics::counter!("interactions_dropped_total", "reason" => "user_install").increment(1);
        tracing::warn!(
            user = ?interaction.author_id(),
            "ignored a command run through a user install"
        );
        return;
    }
    let started = std::time::Instant::now();
    let label = label(&interaction);
    let result = route(&app, &interaction).await;
    metrics::histogram!("interaction_seconds", "kind" => label)
        .record(started.elapsed().as_secs_f64());
    if let Err(error) = result {
        if errors::expired(&error) {
            tracing::warn!(
                kind = label,
                %error,
                "interaction expired before we answered"
            );
            return;
        }
        tracing::error!(kind = label, %error, "interaction failed");
        if interaction.kind == InteractionType::ApplicationCommandAutocomplete {
            return;
        }
        let card = ui::error("ระบบขัดข้อง ลองใหม่อีกครั้ง");
        if reply::edit(&app, &interaction, &card)
            .await
            .is_err()
        {
            let _ = reply::respond(&app, &interaction, card, true).await;
        }
    }
}

/// User installs skip the guild's command permissions.
fn user_installed(interaction: &Interaction) -> bool {
    matches!(
        interaction.data,
        Some(InteractionData::ApplicationCommand(_))
    ) && interaction
        .authorizing_integration_owners
        .guild
        .is_none()
}

fn label(interaction: &Interaction) -> &'static str {
    match &interaction.data {
        Some(InteractionData::ApplicationCommand(data))
            if interaction.kind == InteractionType::ApplicationCommandAutocomplete =>
        {
            match data.name.as_str() {
                "warframe" => "autocomplete_warframe",
                _ => "autocomplete",
            }
        }
        Some(InteractionData::ApplicationCommand(data)) => match data.name.as_str() {
            "admin" => "admin",
            "mod" => "mod",
            "ai" => "ai",
            "warframe" => "warframe",
            "help" => "help",
            "ping" => "ping",
            "intro" => "intro",
            "verify" => "verify",
            _ => "translate",
        },
        Some(InteractionData::MessageComponent(_)) => "component",
        Some(InteractionData::ModalSubmit(_)) => "modal",
        _ => "other",
    }
}

async fn route(app: &Arc<App>, interaction: &Interaction) -> anyhow::Result<()> {
    match &interaction.data {
        Some(InteractionData::ApplicationCommand(data)) => {
            if interaction.kind == InteractionType::ApplicationCommandAutocomplete {
                return commands::autocomplete(app, interaction, data).await;
            }
            if data.kind == CommandType::Message {
                let target = data
                    .target_id
                    .map(|id| id.cast())
                    .and_then(|id| {
                        data.resolved
                            .as_ref()
                            .and_then(|r| r.messages.get(&id))
                    });
                return match target {
                    Some(message) => ai::translate_message(app, interaction, message).await,
                    None => {
                        reply::respond(
                            app,
                            interaction,
                            ui::error("ไม่พบข้อความที่เลือก"),
                            true,
                        )
                        .await
                    }
                };
            }
            commands::dispatch(app, interaction, data).await
        }
        Some(InteractionData::MessageComponent(data)) => {
            let id = data.custom_id.as_str();
            if id.starts_with(roles::PREFIX) {
                roles::on_button(app, interaction, id).await
            } else if id.starts_with(market::PREFIX) {
                market::on_button(app, interaction, id).await
            } else if id.starts_with(status::PREFIX) {
                status::on_button(app, interaction, id).await
            } else if id.starts_with(admin_panel::PREFIX) {
                admin_panel::on_component(app, interaction, data).await
            } else if id.starts_with(quarantine::CASE_PREFIX) {
                quarantine::on_case_button(app, interaction, id).await
            } else if id.starts_with(ai::SHARE_PREFIX) {
                ai::on_share(app, interaction, id).await
            } else {
                Ok(())
            }
        }
        Some(InteractionData::ModalSubmit(data)) => match data.custom_id.as_str() {
            onboarding::INTRO_MODAL => onboarding::submit_intro(app, interaction, data).await,
            onboarding::VERIFY_MODAL => onboarding::submit_verify(app, interaction, data).await,
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(owners: serde_json::Value) -> Interaction {
        let user = serde_json::json!({"id": "7", "username": "u", "discriminator": "0", "avatar": null, "public_flags": 0});
        serde_json::from_value(serde_json::json!({
            "id": "900", "application_id": "1", "type": 2, "token": "t", "version": 1,
            "guild_id": "30", "channel_id": "20", "attachment_size_limit": 10485760,
            "entitlements": [], "authorizing_integration_owners": owners, "context": 0,
            "member": {"user": user, "roles": [], "joined_at": "2026-01-01T00:00:00+00:00",
                "deaf": false, "mute": false, "flags": 0, "permissions": "2048"},
            "data": {"id": "77", "name": "mod", "type": 1}
        }))
        .unwrap()
    }

    #[test]
    fn only_user_installed_commands_are_ignored() {
        assert!(!user_installed(&command(
            serde_json::json!({"0": "30"})
        )));
        assert!(!user_installed(&command(
            serde_json::json!({"0": "30", "1": "7"})
        )));
        assert!(user_installed(&command(
            serde_json::json!({"1": "7"})
        )));
    }
}
