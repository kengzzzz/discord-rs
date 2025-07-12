use anyhow::Context as _;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::{Interaction, application_command::CommandData};

use crate::{context::Context, defer_interaction, services::ai::AiService, utils::embed};
use std::sync::Arc;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "ai", desc_localizations = "ai_desc")]
pub enum AiCommand {
    #[command(name = "prompt")]
    Prompt(AiPromptCommand),
    #[command(name = "talk")]
    Talk(Box<AiTalkCommand>),
    #[command(name = "clear")]
    Clear(AiClearCommand),
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "prompt", desc_localizations = "prompt_desc")]
pub struct AiPromptCommand {
    #[command(desc_localizations = "prompt_prompt_desc")]
    pub prompt: String,
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "talk", desc_localizations = "talk_desc")]
pub struct AiTalkCommand {
    #[command(desc_localizations = "talk_message_desc")]
    pub message: String,
    #[command(desc_localizations = "talk_attachment_desc")]
    pub attachment: Option<twilight_model::channel::Attachment>,
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "clear", desc_localizations = "clear_desc")]
pub struct AiClearCommand {}

fn ai_desc() -> DescLocalizations {
    DescLocalizations::new("AI utilities", [("th", "ผู้ช่วย AI")])
}

fn prompt_desc() -> DescLocalizations {
    DescLocalizations::new("Set your custom prompt", [("th", "ตั้งค่าพรอมพ์ส่วนตัว")])
}

fn prompt_prompt_desc() -> DescLocalizations {
    DescLocalizations::new("Prompt text", [("th", "ข้อความพรอมพ์")])
}

fn talk_desc() -> DescLocalizations {
    DescLocalizations::new("Talk with the AI", [("th", "สนทนากับ AI")])
}

fn talk_message_desc() -> DescLocalizations {
    DescLocalizations::new("Message", [("th", "ข้อความ")])
}

fn talk_attachment_desc() -> DescLocalizations {
    DescLocalizations::new("Attachment", [("th", "ไฟล์แนบ")])
}

fn clear_desc() -> DescLocalizations {
    DescLocalizations::new("Clear your AI chat history", [("th", "ล้างประวัติการสนทนา")])
}

impl AiCommand {
    pub async fn handle(ctx: Arc<Context>, interaction: Interaction, data: CommandData) {
        if let Err(e) = async {
            defer_interaction!(ctx.http, &interaction, true).await?;
            let command =
                AiCommand::from_interaction(data.into()).context("parse ai command data")?;
            match command {
                AiCommand::Prompt(c) => {
                    if let Some(user) = interaction.author() {
                        AiService::set_prompt(ctx.clone(), user.id, c.prompt).await;
                        let embeds = embed::ai_embeds("Prompt updated")?;
                        ctx.http
                            .interaction(interaction.application_id)
                            .update_response(&interaction.token)
                            .embeds(Some(&embeds))
                            .await?;
                    }
                }
                AiCommand::Talk(c) => {
                    let user = interaction.author().context("no author")?;
                    let attachments = c.attachment.clone().into_iter().collect();
                    let reply = AiService::handle_interaction(
                        ctx.clone(),
                        user.id,
                        &user.name,
                        &c.message,
                        attachments,
                    )
                    .await?;
                    let embeds = embed::ai_embeds(&reply)?;
                    ctx.http
                        .interaction(interaction.application_id)
                        .update_response(&interaction.token)
                        .embeds(Some(&embeds))
                        .await?;
                }
                AiCommand::Clear(_) => {
                    if let Some(user) = interaction.author() {
                        AiService::clear_history(user.id).await;
                        let embeds = embed::ai_embeds("History cleared")?;
                        ctx.http
                            .interaction(interaction.application_id)
                            .update_response(&interaction.token)
                            .embeds(Some(&embeds))
                            .await?;
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        }
        .await
        {
            tracing::error!(error=%e, "error handling AiCommand");
        }
    }
}
