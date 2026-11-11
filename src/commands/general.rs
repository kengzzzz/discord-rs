use std::sync::Arc;

use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::{application::interaction::Interaction, channel::message::Component};

use crate::{
    app::App,
    discord::{
        reply,
        ui::{self, Card, color},
    },
};

#[derive(CommandModel, CreateCommand, Debug)]
#[command(
    name = "help",
    desc_localizations = "help_desc",
    integration_types = "guild_install"
)]
pub struct HelpCommand;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(
    name = "ping",
    desc_localizations = "ping_desc",
    integration_types = "guild_install"
)]
pub struct PingCommand;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(
    name = "intro",
    desc_localizations = "intro_desc",
    contexts = "guild",
    integration_types = "guild_install"
)]
pub struct IntroCommand;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(
    name = "verify",
    desc_localizations = "verify_desc",
    contexts = "guild",
    integration_types = "guild_install"
)]
pub struct VerifyCommand;

fn help_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Show what the bot can do",
        [("th", "ดูคำสั่งทั้งหมดของบอท")],
    )
}
fn ping_desc() -> DescLocalizations {
    DescLocalizations::new("Check bot latency", [("th", "ดูความหน่วงของบอท")])
}
fn intro_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Introduce yourself to the server",
        [("th", "แนะนำตัวเพื่อรับสิทธิ์สมาชิก")],
    )
}
fn verify_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Verify you are not a bot",
        [("th", "ยืนยันตัวตนเมื่อถูกกักกัน")],
    )
}

pub fn help_card(bot_name: &str) -> Component {
    Card::new(color::BRAND)
        .title("บอททำอะไรได้บ้าง")
        .text(
            "**Warframe**\n\
             `/warframe market` ราคาจาก warframe.market\n\
             `/warframe build` build จาก overframe.gg\n\
             `/warframe circuit` Incarnon สัปดาห์นี้/ถัดไป\n\
             `/warframe status` สถานะโลก Steel Path Archon Baro",
        )
        .text(format!(
            "**AI**\n\
             แท็กบอท + คำถาม\n\
             แปล: แท็กบอท + `ข้อความ แปล` หรือ reply ข้อความ/รูปด้วย `แปล`\n\
             คลิกขวา → **Apps** → **{}** → **แปลเป็นภาษาไทย** (เห็นเฉพาะคุณ)\n\
             `/ai talk` คุยส่วนตัว · `/ai prompt` ตั้งสไตล์ · `/ai clear` ล้างประวัติ",
            crate::discord::ui::escape_markdown(bot_name)
        ))
        .text("**สมาชิก**\n`/intro` แนะนำตัว · `/verify` ยืนยันตัวตนเมื่อถูกกักกัน")
        .text("**แอดมิน**\n`/admin config` แผงตั้งค่า · `/mod` กักกัน/ปล่อย/ดูเคส")
        .build()
}

pub async fn help(app: &Arc<App>, interaction: &Interaction) -> anyhow::Result<()> {
    let bot_name = app
        .cache
        .current_user()
        .map(|u| u.name.clone())
        .unwrap_or_else(|| "บอท".to_owned());
    reply::respond(app, interaction, vec![help_card(&bot_name)], true).await
}

pub async fn ping(app: &Arc<App>, interaction: &Interaction) -> anyhow::Result<()> {
    let latency = app
        .health
        .latency_ms()
        .map_or_else(|| "—".to_owned(), |ms| format!("{ms} ms"));
    let snapshot_age = app
        .warframe
        .fetched_at()
        .map_or_else(|| "ยังไม่ได้ดึง".to_owned(), |at| ui::ts(at, 'R'));
    let card = Card::new(color::INFO)
        .title("Pong")
        .text(format!(
            "**Gateway** {latency}\n**ออนไลน์ตั้งแต่** {}\n**ข้อมูล Warframe** {snapshot_age}",
            ui::ts(app.started_at * 1000, 'R')
        ))
        .small(concat!("discord-rs v", env!("CARGO_PKG_VERSION")))
        .build();
    reply::respond(app, interaction, vec![card], true).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_card_fits_limits() {
        crate::discord::ui::validate_message(&[help_card(&"ก".repeat(100))]).unwrap();
    }
}
