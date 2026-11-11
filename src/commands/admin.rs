use std::sync::Arc;

use twilight_interactions::command::{
    CommandModel, CreateCommand, DescLocalizations, ResolvedUser,
};
use twilight_model::{application::interaction::Interaction, guild::Permissions, user::User};

use crate::{
    app::App,
    db::{self, quarantine::Reason},
    discord::{
        reply,
        ui::{self, Card, color},
    },
    features::admin_panel,
    moderation::quarantine::{self, Evidence, QuarantineOutcome, ReleaseOutcome, Releaser},
};

#[derive(CommandModel, CreateCommand, Debug)]
#[command(
    name = "admin",
    desc_localizations = "admin_desc",
    default_permissions = "admin_perms",
    contexts = "guild",
    integration_types = "guild_install"
)]
pub enum AdminCommand {
    #[command(name = "config")]
    Config(AdminConfig),
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "config", desc_localizations = "config_desc")]
pub struct AdminConfig;

fn admin_desc() -> DescLocalizations {
    DescLocalizations::new("Bot setup for admins", [("th", "ตั้งค่าบอท")])
}

fn config_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Open the configuration panel",
        [("th", "เปิดแผงตั้งค่า")],
    )
}

fn admin_perms() -> Permissions {
    Permissions::ADMINISTRATOR
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(
    name = "mod",
    desc_localizations = "mod_desc",
    default_permissions = "mod_perms",
    contexts = "guild",
    integration_types = "guild_install"
)]
pub enum ModCommand {
    #[command(name = "quarantine")]
    Quarantine(ModQuarantine),
    #[command(name = "release")]
    Release(ModRelease),
    #[command(name = "cases")]
    Cases(ModCases),
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "quarantine", desc_localizations = "quarantine_desc")]
pub struct ModQuarantine {
    #[command(desc_localizations = "member_desc")]
    pub user: ResolvedUser,
    #[command(desc_localizations = "reason_desc", max_length = 300)]
    pub reason: Option<String>,
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "release", desc_localizations = "release_desc")]
pub struct ModRelease {
    #[command(desc_localizations = "member_desc")]
    pub user: User,
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "cases", desc_localizations = "cases_desc")]
pub struct ModCases;

fn mod_desc() -> DescLocalizations {
    DescLocalizations::new("Moderation tools", [("th", "เครื่องมือทีมงาน")])
}

fn quarantine_desc() -> DescLocalizations {
    DescLocalizations::new("Quarantine a member", [("th", "กักกันสมาชิก")])
}

fn release_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Release a quarantined member",
        [("th", "ปล่อยสมาชิกที่ถูกกักกัน")],
    )
}

fn cases_desc() -> DescLocalizations {
    DescLocalizations::new("List active quarantines", [("th", "ดูเคสที่ค้าง")])
}

fn member_desc() -> DescLocalizations {
    DescLocalizations::new("Member", [("th", "สมาชิก")])
}

fn reason_desc() -> DescLocalizations {
    DescLocalizations::new("Reason", [("th", "เหตุผล")])
}

fn mod_perms() -> Permissions {
    Permissions::ADMINISTRATOR
}

pub async fn run_admin(
    app: &Arc<App>,
    interaction: &Interaction,
    command: AdminCommand,
) -> anyhow::Result<()> {
    match command {
        AdminCommand::Config(_) => admin_panel::open(app, interaction).await,
    }
}

pub async fn run_mod(
    app: &Arc<App>,
    interaction: &Interaction,
    command: ModCommand,
) -> anyhow::Result<()> {
    let (Some(guild), Some(moderator)) = (interaction.guild_id, interaction.author_id()) else {
        return Ok(());
    };
    // Integrations settings can open the command to non-admins.
    if !quarantine::member_permissions(interaction).contains(Permissions::ADMINISTRATOR) {
        return reply::respond(
            app,
            interaction,
            ui::error("คำสั่งนี้สำหรับแอดมินเท่านั้น"),
            true,
        )
        .await;
    }
    match command {
        ModCommand::Quarantine(cmd) => {
            reply::defer(app, interaction, true).await?;
            let target = cmd.user.resolved;
            if target.bot || target.id == moderator {
                reply::edit(
                    app,
                    interaction,
                    &ui::error("กักกันบอทหรือตัวเองไม่ได้"),
                )
                .await?;
                return Ok(());
            }
            let moderator_roles = interaction
                .member
                .as_ref()
                .map_or(&[][..], |m| m.roles.as_slice());
            let target_roles = cmd
                .user
                .member
                .as_ref()
                .map_or(&[][..], |m| m.roles.as_slice());
            if let Err(reason) = quarantine::may_moderate(
                app,
                guild,
                (moderator, moderator_roles),
                (target.id, target_roles),
            ) {
                let card = ui::notice(ui::color::WARNING, "กักกันไม่ได้", reason);
                reply::edit(app, interaction, &card).await?;
                return Ok(());
            }
            let evidence = Evidence {
                summary: cmd.reason.unwrap_or_default(),
                excerpt: String::new(),
                channel: interaction
                    .channel
                    .as_ref()
                    .map(|c| c.id),
            };
            let card = match quarantine::quarantine(
                app,
                guild,
                &target,
                Reason::Manual,
                evidence,
                Some((moderator, moderator_roles)),
            )
            .await
            {
                Ok(QuarantineOutcome::Quarantined) => {
                    ui::success("กักกันแล้ว", &format!("<@{}> ถูกกักกัน", target.id))
                }
                Ok(QuarantineOutcome::AlreadyQuarantined) => ui::notice(
                    color::MUTED,
                    "ถูกกักกันอยู่แล้ว",
                    &format!("<@{}>", target.id),
                ),
                Ok(QuarantineOutcome::Upgraded) => ui::success(
                    "เปลี่ยนเป็นเคสทีมงานแล้ว",
                    &format!("<@{}> ต้องให้ทีมงานปล่อยเท่านั้น", target.id),
                ),
                Ok(QuarantineOutcome::Refused(reason)) => {
                    ui::notice(ui::color::WARNING, "กักกันไม่ได้", reason)
                }
                Ok(QuarantineOutcome::NotConfigured) => {
                    ui::error("ยังไม่ได้ตั้งค่า role/ช่องกักกัน — ใช้ `/admin config`")
                }
                Err(error) => {
                    tracing::warn!(%error, "manual quarantine failed");
                    ui::error("กักกันไม่สำเร็จ (ตรวจสอบสิทธิ์และลำดับ role ของบอท)")
                }
            };
            reply::edit(app, interaction, &card).await?;
        }
        ModCommand::Release(cmd) => {
            reply::defer(app, interaction, true).await?;
            let card = match quarantine::release(
                app,
                guild,
                cmd.user.id,
                Releaser::Moderator {
                    by: moderator,
                    roles: quarantine::member_roles(interaction),
                    card: None,
                },
            )
            .await?
            {
                ReleaseOutcome::Released => ui::success(
                    "ปล่อยแล้ว",
                    &format!("คืน role ให้ <@{}> แล้ว", cmd.user.id),
                ),
                ReleaseOutcome::ClosedGone => quarantine::closed_gone(cmd.user.id),
                ReleaseOutcome::NotQuarantined => ui::notice(
                    color::MUTED,
                    "ไม่ได้ถูกกักกัน",
                    &format!("<@{}>", cmd.user.id),
                ),
                ReleaseOutcome::WrongToken
                | ReleaseOutcome::StaffOnly
                | ReleaseOutcome::StaleCard => ui::error("token ไม่ถูกต้อง"),
                ReleaseOutcome::Failed(reason) => ui::error(&reason),
            };
            reply::edit(app, interaction, &card).await?;
        }
        ModCommand::Cases(_) => {
            reply::defer(app, interaction, true).await?;
            let records = app
                .db
                .call(db::quarantine::list)
                .await?;
            let mut open: Vec<_> = records
                .iter()
                .filter(|r| r.guild_id == guild)
                .collect();
            open.sort_unstable_by_key(|r| std::cmp::Reverse(r.created_at));
            let lines: Vec<String> = open
                .iter()
                .take(25)
                .map(|r| {
                    let summary = r
                        .evidence
                        .lines()
                        .next()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(quarantine::reason_label(r.reason));
                    format!(
                        "<@{}> · {} · {}",
                        r.user_id,
                        ui::truncate(summary, 80),
                        ui::ts(r.created_at * 1000, 'R')
                    )
                })
                .collect();
            let body = if lines.is_empty() {
                "ไม่มีเคสค้าง".to_owned()
            } else {
                lines.join("\n")
            };
            let card = Card::new(color::INFO)
                .title(format!("เคสกักกันที่ค้างอยู่ ({})", open.len()))
                .text(body)
                .small("ปล่อยด้วย `/mod release` หรือปุ่มในการ์ดเคส")
                .build();
            reply::edit(app, interaction, &[card]).await?;
        }
    }
    Ok(())
}
