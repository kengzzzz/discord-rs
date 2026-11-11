use std::sync::Arc;

use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::{application::interaction::Interaction, channel::Attachment};

use crate::{
    ai,
    app::App,
    db,
    discord::{
        reply,
        ui::{self, Card, color},
    },
};

#[derive(CommandModel, CreateCommand, Debug)]
#[command(
    name = "ai",
    desc_localizations = "ai_desc",
    contexts = "guild",
    integration_types = "guild_install"
)]
pub enum AiCommand {
    #[command(name = "talk")]
    Talk(Box<AiTalk>),
    #[command(name = "prompt")]
    Prompt(AiPrompt),
    #[command(name = "clear")]
    Clear(AiClear),
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "talk", desc_localizations = "talk_desc")]
pub struct AiTalk {
    #[command(desc_localizations = "message_desc", max_length = 4000)]
    pub message: String,
    #[command(desc_localizations = "attachment_desc")]
    pub attachment: Option<Attachment>,
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "prompt", desc_localizations = "prompt_desc")]
pub enum AiPrompt {
    #[command(name = "set")]
    Set(AiPromptSet),
    #[command(name = "show")]
    Show(AiPromptShow),
    #[command(name = "clear")]
    Clear(AiPromptClear),
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "set", desc_localizations = "prompt_set_desc")]
pub struct AiPromptSet {
    #[command(desc_localizations = "prompt_text_desc", max_length = 1000)]
    pub text: String,
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "show", desc_localizations = "prompt_show_desc")]
pub struct AiPromptShow;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "clear", desc_localizations = "prompt_clear_desc")]
pub struct AiPromptClear;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "clear", desc_localizations = "clear_desc")]
pub struct AiClear;

fn ai_desc() -> DescLocalizations {
    DescLocalizations::new("AI assistant", [("th", "ผู้ช่วย AI")])
}
fn talk_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Chat privately with the AI",
        [("th", "คุยกับ AI แบบส่วนตัว (เห็นเฉพาะคุณ)")],
    )
}
fn message_desc() -> DescLocalizations {
    DescLocalizations::new("Message", [("th", "ข้อความ")])
}
fn attachment_desc() -> DescLocalizations {
    DescLocalizations::new("Image or PDF", [("th", "รูปภาพหรือ PDF")])
}
fn prompt_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Your personal AI style",
        [("th", "สไตล์การตอบของ AI")],
    )
}
fn prompt_set_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Set your personal AI style",
        [("th", "ตั้งสไตล์การตอบ")],
    )
}
fn prompt_text_desc() -> DescLocalizations {
    DescLocalizations::new(
        "How the AI should talk to you",
        [("th", "อยากให้ AI ตอบแบบไหน")],
    )
}
fn prompt_show_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Show your personal AI style",
        [("th", "ดูสไตล์ที่ตั้งไว้")],
    )
}
fn prompt_clear_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Remove your personal AI style",
        [("th", "ลบสไตล์ที่ตั้งไว้")],
    )
}
fn clear_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Clear your AI chat history",
        [("th", "ล้างประวัติการคุยกับ AI")],
    )
}

/// Imported prompts can be longer than the 1000 characters `/ai prompt set` allows.
fn prompt_card(prompt: Option<&str>) -> Vec<twilight_model::channel::message::Component> {
    let body = prompt.map_or_else(
        || "ยังไม่ได้ตั้งสไตล์".to_owned(),
        |p| {
            format!(
                ">>> {}",
                ui::truncate(&ui::escape_markdown(p), 3000)
            )
        },
    );
    vec![
        Card::new(color::AI)
            .title("สไตล์ AI ของคุณ")
            .text(body)
            .build(),
    ]
}

pub async fn run(
    app: &Arc<App>,
    interaction: &Interaction,
    command: AiCommand,
) -> anyhow::Result<()> {
    let Some(user) = interaction.author_id() else { return Ok(()) };
    match command {
        AiCommand::Talk(talk) => {
            let AiTalk { message, attachment } = *talk;
            ai::talk(app, interaction, message, attachment).await
        }
        AiCommand::Prompt(AiPrompt::Set(set)) => {
            ai::set_prompt(app, user, set.text.trim().to_owned()).await?;
            reply::respond(
                app,
                interaction,
                ui::success("ตั้งสไตล์แล้ว", "ล้างประวัติเก่าให้แล้ว"),
                true,
            )
            .await
        }
        AiCommand::Prompt(AiPrompt::Show(_)) => {
            let prompt = app
                .db
                .call(move |c| db::ai::get_prompt(c, user))
                .await?;
            reply::respond(
                app,
                interaction,
                prompt_card(prompt.as_deref()),
                true,
            )
            .await
        }
        AiCommand::Prompt(AiPrompt::Clear(_)) => {
            let removed = app
                .db
                .call(move |c| db::ai::clear_prompt(c, user))
                .await?;
            let text = if removed {
                "ลบสไตล์แล้ว"
            } else {
                "ยังไม่ได้ตั้งสไตล์"
            };
            reply::respond(
                app,
                interaction,
                ui::notice(color::MUTED, text, "AI จะตอบด้วยสไตล์ปกติ"),
                true,
            )
            .await
        }
        AiCommand::Clear(_) => {
            app.ai.history_cleared(user);
            let removed = app
                .db
                .call(move |c| db::ai::clear_turns(c, user))
                .await?;
            reply::respond(
                app,
                interaction,
                ui::success("ล้างประวัติแล้ว", &format!("ลบ {removed} ข้อความ")),
                true,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_imported_prompt_fits_one_message() {
        ui::validate_message(&prompt_card(Some(&"*_".repeat(2500)))).unwrap();
        ui::validate_message(&prompt_card(None)).unwrap();
    }
}
