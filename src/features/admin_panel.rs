use std::{sync::Arc, time::Duration};

use twilight_model::{
    application::interaction::{Interaction, message_component::MessageComponentInteractionData},
    channel::{
        ChannelType,
        message::{
            Component,
            component::{ActionRow, ButtonStyle, SelectDefaultValue, SelectMenu, SelectMenuType},
        },
    },
    guild::Permissions,
    id::{
        Id,
        marker::{GuildMarker, RoleMarker, UserMarker},
    },
};

use crate::{
    app::App,
    discord::{
        reply,
        ui::{self, Card, color},
    },
    features::roles,
    guild::{ChannelKind, RoleBinding, RoleKind},
    warframe,
};

pub const PREFIX: &str = "cfg:";

/// Stops two panel changes from binding one role to two kinds.
static ROLE_BINDINGS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Channels,
    SystemRoles,
    PickableRoles,
}

impl Page {
    const ALL: [Self; 4] = [Self::Overview, Self::Channels, Self::SystemRoles, Self::PickableRoles];

    const fn id(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Channels => "channels",
            Self::SystemRoles => "system",
            Self::PickableRoles => "pickable",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Overview => "ภาพรวม",
            Self::Channels => "ช่อง",
            Self::SystemRoles => "Role ระบบ",
            Self::PickableRoles => "Role ที่เลือกเองได้",
        }
    }

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|p| p.id() == value)
            .unwrap_or(Self::Overview)
    }
}

const SYSTEM_ROLES: [RoleKind; 3] = [RoleKind::Guest, RoleKind::Member, RoleKind::Quarantine];

fn channel_hint(kind: ChannelKind) -> &'static str {
    match kind {
        ChannelKind::Notification => "แจ้งเตือน Helminth, Riven, Umbra, Baro, Cetus",
        ChannelKind::Status => "การ์ดสถานะ Warframe",
        ChannelKind::Introduction => "โพสต์แนะนำตัวจาก `/intro`",
        ChannelKind::RolePicker => "การ์ดปุ่มเลือก role",
        ChannelKind::Relay => "ส่งต่อข้อความข้ามเซิร์ฟเวอร์ (กลุ่มหลัก)",
        ChannelKind::RelayB1 => "ส่งต่อข้อความข้ามเซิร์ฟเวอร์ (กลุ่ม 1)",
        ChannelKind::RelayB2 => "ส่งต่อข้อความข้ามเซิร์ฟเวอร์ (กลุ่ม 2)",
        ChannelKind::Quarantine => "ผู้ถูกกักกันใช้ `/verify` ที่นี่",
        ChannelKind::VoiceLog => "บันทึกการเข้า/ออกห้องเสียง",
        ChannelKind::ModLog => "การ์ดเคสกักกันสำหรับทีมงาน",
    }
}

fn role_hint(kind: RoleKind) -> &'static str {
    match kind {
        RoleKind::Guest => "ให้อัตโนมัติเมื่อเข้าเซิร์ฟเวอร์",
        RoleKind::Member => "ได้หลัง `/intro` (แทน Guest)",
        RoleKind::Quarantine => "ใช้ตอนกักกัน",
        other => other.hint(),
    }
}

fn select(
    kind: SelectMenuType,
    custom_id: String,
    current: Option<SelectDefaultValue>,
    placeholder: &str,
) -> Component {
    Component::ActionRow(ActionRow {
        id: None,
        components: vec![Component::SelectMenu(SelectMenu {
            channel_types: (kind == SelectMenuType::Channel)
                .then(|| vec![ChannelType::GuildText, ChannelType::GuildAnnouncement]),
            custom_id,
            default_values: current.map(|v| vec![v]),
            disabled: false,
            id: None,
            kind,
            max_values: Some(1),
            min_values: Some(0),
            options: None,
            placeholder: Some(placeholder.to_owned()),
            required: None,
        })],
    })
}

fn nav(page: Page) -> Vec<Component> {
    Page::ALL
        .into_iter()
        .map(|p| {
            let style = if p == page { ButtonStyle::Primary } else { ButtonStyle::Secondary };
            ui::button_with(
                style,
                format!("{PREFIX}page:{}", p.id()),
                p.label(),
                p == page,
            )
        })
        .collect()
}

fn bot_top_position(app: &App, guild: Id<GuildMarker>) -> i64 {
    app.bot_top_position(guild)
        .unwrap_or(i64::MAX)
}

pub fn warnings(app: &App, guild: Id<GuildMarker>) -> Vec<String> {
    let config = app.guild(guild);
    let mut out = Vec::new();
    let permissions = app.cache.permissions();
    if let Ok(root) = permissions.root(app.bot_id, guild) {
        for (perm, label) in [
            (Permissions::MANAGE_ROLES, "Manage Roles"),
            (Permissions::MANAGE_MESSAGES, "Manage Messages"),
            (Permissions::KICK_MEMBERS, "Kick Members"),
        ] {
            if !root.contains(perm) {
                out.push(format!("บอทไม่มีสิทธิ์ **{label}**"));
            }
        }
    }
    for kind in ChannelKind::ALL {
        let Some(channel) = config.channel(kind) else {
            continue;
        };
        match permissions.in_channel(app.bot_id, channel) {
            Ok(perms) if perms.contains(Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES) => {
            }
            Ok(_) => out.push(format!(
                "{}: บอทส่งข้อความใน <#{channel}> ไม่ได้",
                kind.label()
            )),
            // Threads aren't cached, so a thread binding lands here too.
            Err(_) => out.push(format!(
                "{}: ไม่พบช่อง <#{channel}> (ถ้าเป็นเธรด บอทตรวจสิทธิ์ไม่ได้)",
                kind.label()
            )),
        }
    }
    if config
        .role_id(RoleKind::Quarantine)
        .is_none()
        || config
            .channel(ChannelKind::Quarantine)
            .is_none()
    {
        out.push("ยังไม่ได้ตั้ง role/ช่องกักกัน: ตรวจจับสแปมและภาพหลอกลวงปิดอยู่".to_owned());
    } else if config
        .channel(ChannelKind::ModLog)
        .is_none()
    {
        out.push("ยังไม่ได้ตั้งช่อง Moderation log: ทีมงานจะไม่เห็นการ์ดเคส".to_owned());
    }
    let top = bot_top_position(app, guild);
    for kind in RoleKind::ALL {
        let Some(binding) = config.role(kind) else {
            continue;
        };
        match app.cache.role(binding.id) {
            Some(role) if role.position >= top => {
                out.push(format!("<@&{}> อยู่สูงกว่าบอท จัดการไม่ได้", binding.id));
            }
            Some(_) if roles::grants_staff(app, guild, binding.id) => {
                out.push(format!(
                    "<@&{}> มีสิทธิ์ทีมงาน บอทจะไม่แจก role นี้",
                    binding.id
                ));
            }
            Some(_) => {}
            None => out.push(format!(
                "{}: ไม่พบ role <@&{}>",
                kind.label(),
                binding.id
            )),
        }
    }
    out
}

pub fn render(
    app: &App,
    guild: Id<GuildMarker>,
    page: Page,
    banner: Option<&str>,
) -> Vec<Component> {
    let config = app.guild(guild);
    let mut header = format!("### ตั้งค่าบอท · {}", app.guild_name(guild));
    if page != Page::Overview {
        header.push_str("\n-# เลือกเพื่อตั้งค่า · เอาออกเพื่อยกเลิก");
    }
    if let Some(banner) = banner {
        header.push('\n');
        header.push_str(banner);
    }
    let mut card = Card::new(color::INFO)
        .text(header)
        .buttons(nav(page))
        .divider();

    card = match page {
        Page::Overview => {
            let mut lines = Vec::new();
            for kind in ChannelKind::ALL {
                let value = config
                    .channel(kind)
                    .map_or_else(|| "—".to_owned(), |c| format!("<#{c}>"));
                lines.push(format!("{} · {value}", kind.label()));
            }
            let mut role_lines = Vec::new();
            for kind in RoleKind::ALL {
                let value = config
                    .role_id(kind)
                    .map_or_else(|| "—".to_owned(), |r| format!("<@&{r}>"));
                role_lines.push(format!("{} · {value}", kind.label()));
            }
            let scam = if !app.config.scam.enabled {
                "ปิดโดยผู้ดูแลบอท"
            } else if config.scam_detect {
                "เปิด"
            } else {
                "ปิด"
            };
            let warnings = warnings(app, guild);
            let mut card = card
                .text(format!("**ช่อง**\n{}", lines.join("\n")))
                .text(format!("**Role**\n{}", role_lines.join("\n")))
                .text(format!("**ตรวจจับภาพหลอกลวง (OCR):** {scam}"));
            if !warnings.is_empty() {
                card = card
                    .divider()
                    .text(format!("**ควรแก้ไข**\n{}", warnings.join("\n")));
            }
            card.divider().buttons([
                ui::button_with(
                    if config.scam_detect { ButtonStyle::Danger } else { ButtonStyle::Success },
                    format!(
                        "{PREFIX}scam:{}",
                        if config.scam_detect { "off" } else { "on" }
                    ),
                    if config.scam_detect {
                        "ปิดตรวจจับภาพ"
                    } else {
                        "เปิดตรวจจับภาพ"
                    },
                    !app.config.scam.enabled,
                ),
                ui::button_with(
                    ButtonStyle::Secondary,
                    format!("{PREFIX}repost"),
                    "โพสต์การ์ดใหม่",
                    false,
                ),
            ])
        }
        Page::Channels => {
            let mut card = card;
            for kind in ChannelKind::ALL
                .into_iter()
                .filter(|k| !k.is_relay())
            {
                card = card.text(format!(
                    "**{}** — {}",
                    kind.label(),
                    channel_hint(kind)
                ));
                card = card.raw(select(
                    SelectMenuType::Channel,
                    format!("{PREFIX}ch:{}", kind.as_str()),
                    // A default Discord refuses (deleted, or a voice chat) would break the panel.
                    config
                        .channel(kind)
                        .filter(|id| {
                            app.cache.channel(*id).is_some_and(|c| {
                                matches!(
                                    c.kind,
                                    ChannelType::GuildText | ChannelType::GuildAnnouncement
                                )
                            })
                        })
                        .map(SelectDefaultValue::Channel),
                    "ยังไม่ได้ตั้งค่า",
                ));
            }
            card
        }
        Page::SystemRoles | Page::PickableRoles => {
            let kinds: &[RoleKind] =
                if page == Page::SystemRoles { &SYSTEM_ROLES } else { &RoleKind::PICKABLE };
            let mut card = card;
            for kind in kinds {
                card = card.text(format!(
                    "**{}** — {}",
                    kind.label(),
                    role_hint(*kind)
                ));
                card = card.raw(select(
                    SelectMenuType::Role,
                    format!("{PREFIX}role:{}", kind.as_str()),
                    config
                        .role_id(*kind)
                        .filter(|id| app.cache.role(*id).is_some())
                        .map(SelectDefaultValue::Role),
                    "ยังไม่ได้ตั้งค่า",
                ));
            }
            card
        }
    };
    vec![card.build()]
}

fn can_manage(interaction: &Interaction) -> bool {
    interaction
        .member
        .as_ref()
        .and_then(|m| m.permissions)
        .is_some_and(|p| p.contains(Permissions::ADMINISTRATOR))
}

pub async fn open(app: &Arc<App>, interaction: &Interaction) -> anyhow::Result<()> {
    let Some(guild) = interaction.guild_id else {
        return Ok(());
    };
    // Integrations settings can open the command to anyone.
    if !can_manage(interaction) {
        return reply::respond(
            app,
            interaction,
            ui::error("คำสั่งนี้สำหรับแอดมินเท่านั้น"),
            true,
        )
        .await;
    }
    reply::respond(
        app,
        interaction,
        render(app, guild, Page::Overview, None),
        true,
    )
    .await
}

pub async fn on_component(
    app: &Arc<App>,
    interaction: &Interaction,
    data: &MessageComponentInteractionData,
) -> anyhow::Result<()> {
    let Some(guild) = interaction.guild_id else {
        return Ok(());
    };
    if !can_manage(interaction) {
        return reply::respond(
            app,
            interaction,
            ui::error("คำสั่งนี้สำหรับแอดมินเท่านั้น"),
            true,
        )
        .await;
    }
    // Role changes can wait on locks held through Discord calls, so acknowledge first.
    reply::defer_update(app, interaction).await?;
    let action = data
        .custom_id
        .strip_prefix(PREFIX)
        .unwrap_or_default();
    let (page, banner) = match action.split_once(':') {
        Some(("page", page)) => (Page::parse(page), None),
        Some(("ch", kind)) => {
            // The bot is public, so any server's admin could join the relay network; bindings come
            // only from `migrate` until relay gets its own allow-list.
            let Some(kind) = ChannelKind::parse(kind).filter(|k| !k.is_relay()) else {
                return Ok(());
            };
            (
                Page::Channels,
                Some(apply_channel(app, guild, kind, data.values.first()).await?),
            )
        }
        Some(("role", kind)) => {
            let Some(kind) = RoleKind::parse(kind) else { return Ok(()) };
            let page =
                if SYSTEM_ROLES.contains(&kind) { Page::SystemRoles } else { Page::PickableRoles };
            let invoker = (
                interaction.author_id(),
                interaction
                    .member
                    .as_ref()
                    .map_or(&[][..], |m| m.roles.as_slice()),
            );
            (
                page,
                Some(apply_role(app, guild, invoker, kind, data.values.first()).await?),
            )
        }
        // The wanted state, not a toggle: another admin may have changed it.
        Some(("scam", state @ ("on" | "off"))) => {
            let enabled = state == "on";
            app.guilds
                .set_scam_detect(guild, enabled)
                .await?;
            (
                Page::Overview,
                Some(format!(
                    "ตรวจจับภาพหลอกลวง: **{}**",
                    if enabled { "เปิด" } else { "ปิด" }
                )),
            )
        }
        None if action == "repost" => {
            let app2 = Arc::clone(app);
            app.spawn(async move {
                roles::ensure_picker(&app2, guild).await;
                warframe::status::refresh_guild(&app2, guild, true).await;
            });
            (Page::Overview, Some("กำลังโพสต์การ์ดใหม่".to_owned()))
        }
        _ => return Ok(()),
    };
    reply::edit(
        app,
        interaction,
        &render(app, guild, page, banner.as_deref()),
    )
    .await?;
    Ok(())
}

async fn apply_channel(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    kind: ChannelKind,
    value: Option<&String>,
) -> anyhow::Result<String> {
    let current = app.guild(guild).channel(kind);
    let banner = match value
        .and_then(|v| v.parse::<u64>().ok())
        .and_then(Id::new_checked)
    {
        // Values come from the client; accept only this guild's text channels.
        Some(channel)
            if app
                .cache
                .channel(channel)
                .is_none_or(|c| {
                    c.guild_id != Some(guild)
                        || !matches!(
                            c.kind,
                            ChannelType::GuildText | ChannelType::GuildAnnouncement
                        )
                }) =>
        {
            "ไม่พบช่องนี้ในเซิร์ฟเวอร์ ลองใหม่อีกครั้ง".to_owned()
        }
        Some(channel) => {
            app.guilds
                .set_channel(guild, kind, channel)
                .await?;
            format!("ตั้งค่าแล้ว: {} → <#{channel}>", kind.label())
        }
        None => {
            if let Some(channel) = current {
                app.guilds
                    .clear_kind(guild, kind)
                    .await?;
                format!("ยกเลิกแล้ว: {} (เดิม <#{channel}>)", kind.label())
            } else {
                format!("{} ยังไม่ได้ตั้งค่า", kind.label())
            }
        }
    };
    let app2 = Arc::clone(app);
    match kind {
        ChannelKind::RolePicker => {
            app.spawn(async move { roles::ensure_picker(&app2, guild).await })
        }
        ChannelKind::Status => {
            app.spawn(async move { warframe::status::refresh_guild(&app2, guild, true).await })
        }
        _ => {}
    }
    Ok(banner)
}

fn binding_refusal(
    app: &App,
    guild: Id<GuildMarker>,
    (invoker, invoker_roles): (Option<Id<UserMarker>>, &[Id<RoleMarker>]),
    kind: RoleKind,
    role: Id<RoleMarker>,
) -> Option<String> {
    let config = app.guild(guild);
    if let Some(other) = RoleKind::ALL
        .into_iter()
        .find(|k| *k != kind && config.role_id(*k) == Some(role))
    {
        return Some(format!("role นี้ใช้เป็น {} อยู่แล้ว", other.label()));
    }
    let Some(position) = app
        .cache
        .role(role)
        .filter(|r| r.guild_id() == guild)
        .map(|r| r.position)
    else {
        return Some("ไม่พบ role นี้ ลองใหม่อีกครั้ง".to_owned());
    };
    if roles::grants_staff(app, guild, role) {
        return Some("role นี้มีสิทธิ์ทีมงาน ใช้ไม่ได้".to_owned());
    }
    let owner = app
        .cache
        .guild(guild)
        .map(|g| g.owner_id());
    if invoker.is_none() || invoker != owner {
        let top = invoker_roles
            .iter()
            .filter_map(|r| app.cache.role(*r).map(|r| r.position))
            .max()
            .unwrap_or(0);
        if position >= top {
            return Some("role นี้สูงเท่าหรือสูงกว่า role ของคุณ".to_owned());
        }
    }
    None
}

async fn apply_role(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    invoker: (Option<Id<UserMarker>>, &[Id<RoleMarker>]),
    kind: RoleKind,
    value: Option<&String>,
) -> anyhow::Result<String> {
    let role: Option<Id<RoleMarker>> = value
        .and_then(|v| v.parse::<u64>().ok())
        .and_then(Id::new_checked);
    let _bindings = ROLE_BINDINGS.lock().await;
    // Startup releases anyone without the quarantine role, so switching it releases every case.
    let _config = if kind == RoleKind::Quarantine {
        // Quarantines in progress can sit on Discord timeouts.
        match tokio::time::timeout(
            Duration::from_secs(10),
            app.moderation.quarantine_config.write(),
        )
        .await
        {
            Ok(guard) => Some(guard),
            Err(_) => return Ok("กำลังกักกันสมาชิกอยู่ ลองใหม่อีกครั้ง".to_owned()),
        }
    } else {
        None
    };
    // A deleted role whose RoleDelete was missed still counts as deleted.
    let previous = app
        .guild(guild)
        .role_id(RoleKind::Quarantine)
        .filter(|old| crate::moderation::quarantine::role_exists(app, guild, *old));
    // With the old role deleted, open cases are unrestricted, so accept a new one and grant it.
    if kind == RoleKind::Quarantine && previous.is_some() && role != previous {
        let open = app
            .db
            .call(crate::db::quarantine::list)
            .await?
            .into_iter()
            .filter(|r| r.guild_id == guild)
            .count();
        if open > 0 {
            return Ok(format!(
                "มีเคสกักกันค้าง {open} เคส ปล่อยก่อน (แอดมินดูได้ที่ `/mod cases`) แล้วค่อยเปลี่ยน role กักกัน"
            ));
        }
    }
    let banner = match role {
        Some(role) if role.get() == guild.get() => return Ok("ใช้ @everyone ไม่ได้".to_owned()),
        Some(role)
            if app
                .cache
                .role(role)
                .is_some_and(|r| r.managed) =>
        {
            return Ok("role ของบอท/integration ใช้ไม่ได้".to_owned());
        }
        Some(role) if let Some(refusal) = binding_refusal(app, guild, invoker, kind, role) => {
            return Ok(refusal);
        }
        Some(role) => {
            let replacing_deleted = kind == RoleKind::Quarantine && previous.is_none();
            if replacing_deleted {
                // Marked first, so a restart midway re-applies instead of reading a staff removal.
                app.db
                    .call(move |c| crate::db::quarantine::mark_active_for_reapply(c, guild))
                    .await?;
            }
            let binding =
                RoleBinding { id: role, self_assignable: RoleKind::PICKABLE.contains(&kind) };
            app.guilds
                .set_role(guild, kind, binding)
                .await?;
            let above = app
                .cache
                .role(role)
                .is_some_and(|r| r.position >= bot_top_position(app, guild));
            let mut text = format!("ตั้งค่าแล้ว: {} → <@&{role}>", kind.label());
            if replacing_deleted {
                let app2 = Arc::clone(app);
                app.spawn(async move {
                    crate::moderation::quarantine::reapply_open_cases(&app2, guild, role).await;
                });
            }
            if above {
                text.push_str("\nrole นี้อยู่สูงกว่าบอท ย้าย role บอทขึ้นไปก่อน");
            }
            text
        }
        None => {
            if app.guild(guild).role(kind).is_some() {
                app.guilds
                    .clear_role_kind(guild, kind)
                    .await?;
                format!("ยกเลิกแล้ว: {}", kind.label())
            } else {
                format!("{} ยังไม่ได้ตั้งค่า", kind.label())
            }
        }
    };
    if RoleKind::PICKABLE.contains(&kind) {
        let app2 = Arc::clone(app);
        app.spawn(async move { roles::ensure_picker(&app2, guild).await });
    }
    Ok(banner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, discord::ui::validate_message};

    #[tokio::test]
    async fn every_page_fits_discord_limits_when_fully_configured() {
        let app = test_app().await;
        let guild = Id::new(77);
        for (index, kind) in ChannelKind::ALL.into_iter().enumerate() {
            app.guilds
                .set_channel(
                    guild,
                    kind,
                    Id::new(1_000_000_000_000_000_000 + index as u64),
                )
                .await
                .unwrap();
        }
        for (index, kind) in RoleKind::ALL.into_iter().enumerate() {
            let binding = RoleBinding {
                id: Id::new(2_000_000_000_000_000_000 + index as u64),
                self_assignable: true,
            };
            app.guilds
                .set_role(guild, kind, binding)
                .await
                .unwrap();
        }
        let banner = format!("✅ {}", "ก".repeat(200));
        for page in Page::ALL {
            for banner in [None, Some(banner.as_str())] {
                let components = render(&app, guild, page, banner);
                validate_message(&components).unwrap_or_else(|e| panic!("{}: {e}", page.id()));
            }
        }
    }

    #[tokio::test]
    async fn channels_page_offers_no_relay_binding() {
        let app = test_app().await;
        let guild = Id::new(77);
        app.guilds
            .set_channel(guild, ChannelKind::Relay, Id::new(10))
            .await
            .unwrap();
        let json = serde_json::to_string(&render(&app, guild, Page::Channels, None)).unwrap();
        assert!(json.contains(&format!(
            "{PREFIX}ch:{}",
            ChannelKind::Status.as_str()
        )));
        for kind in ChannelKind::ALL
            .into_iter()
            .filter(|k| k.is_relay())
        {
            assert!(
                !json.contains(&format!("{PREFIX}ch:{}\"", kind.as_str())),
                "{kind:?}"
            );
        }
    }

    fn cache_role(app: &App, guild: Id<GuildMarker>, id: u64, position: i64, permissions: u64) {
        use twilight_model::gateway::{event::Event, payload::incoming::RoleCreate};
        let role = serde_json::from_value(serde_json::json!({
            "id": id.to_string(), "name": format!("r{id}"), "color": 0, "hoist": false,
            "colors": {"primary_color": 0},
            "position": position, "permissions": permissions.to_string(), "managed": false,
            "mentionable": false, "flags": 0
        }))
        .unwrap();
        app.cache
            .update(&Event::RoleCreate(RoleCreate {
                guild_id: guild,
                role,
            }));
    }

    #[tokio::test]
    async fn staff_duplicate_and_higher_roles_cannot_be_bound() {
        let app = test_app().await;
        let guild = Id::new(77);
        for (id, position, permissions) in [
            (77u64, 0, Permissions::MENTION_EVERYONE.bits()),
            (501, 3, Permissions::MENTION_EVERYONE.bits()),
            (502, 4, Permissions::KICK_MEMBERS.bits()),
            (503, 12, 0),
            (504, 2, Permissions::MODERATE_MEMBERS.bits()),
            (510, 10, 0),
        ] {
            cache_role(&app, guild, id, position, permissions);
        }
        let invoker = (Some(Id::new(9)), &[Id::new(510)][..]);
        let refusal = |kind, role: u64| binding_refusal(&app, guild, invoker, kind, Id::new(role));
        assert_eq!(refusal(RoleKind::Helminth, 501), None);
        assert!(
            refusal(RoleKind::Helminth, 502)
                .unwrap()
                .contains("สิทธิ์ทีมงาน")
        );
        assert!(
            refusal(RoleKind::Member, 502)
                .unwrap()
                .contains("สิทธิ์ทีมงาน")
        );
        assert!(
            refusal(RoleKind::Helminth, 503)
                .unwrap()
                .contains("สูง")
        );
        assert!(
            refusal(RoleKind::Helminth, 999)
                .unwrap()
                .contains("ไม่พบ")
        );
        app.guilds
            .set_role(
                guild,
                RoleKind::Quarantine,
                RoleBinding { id: Id::new(501), self_assignable: false },
            )
            .await
            .unwrap();
        assert!(
            refusal(RoleKind::Helminth, 501)
                .unwrap()
                .contains("ใช้เป็น")
        );
        assert_eq!(refusal(RoleKind::Quarantine, 501), None);
        assert!(!roles::grants_staff(&app, guild, Id::new(501)));
        assert!(roles::grants_staff(&app, guild, Id::new(502)));
        assert!(roles::grants_staff(&app, guild, Id::new(504)));
        assert!(roles::grants_staff(&app, guild, Id::new(999)));
    }

    #[tokio::test]
    async fn one_role_cannot_be_bound_twice_by_concurrent_changes() {
        let app = test_app().await;
        let guild = Id::new(77);
        cache_role(&app, guild, 501, 3, 0);
        cache_role(&app, guild, 510, 10, 0);
        let invoker = (Some(Id::new(9)), &[Id::new(510)][..]);
        let role = "501".to_owned();
        let (guest, member) = tokio::join!(
            apply_role(&app, guild, invoker, RoleKind::Guest, Some(&role)),
            apply_role(
                &app,
                guild,
                invoker,
                RoleKind::Member,
                Some(&role)
            ),
        );
        let banners = [guest.unwrap(), member.unwrap()];
        assert_eq!(
            banners
                .iter()
                .filter(|b| b.contains("ตั้งค่าแล้ว"))
                .count(),
            1,
            "{banners:?}"
        );
        assert!(
            banners
                .iter()
                .any(|b| b.contains("ใช้เป็น")),
            "{banners:?}"
        );
    }

    #[tokio::test]
    async fn quarantine_role_cannot_move_while_cases_are_open() {
        use crate::db::quarantine::{Reason, Record, State};
        let app = test_app().await;
        let guild = Id::new(77);
        let current = Id::new(500);
        cache_role(&app, guild, 500, 2, 0);
        cache_role(&app, guild, 510, 10, 0);
        let invoker = (Some(Id::new(9)), &[Id::new(510)][..]);
        app.guilds
            .set_role(
                guild,
                RoleKind::Quarantine,
                RoleBinding { id: current, self_assignable: false },
            )
            .await
            .unwrap();
        let record = Record {
            guild_id: guild,
            user_id: Id::new(9),
            token: "123456".into(),
            roles: vec![Id::new(600)],
            state: State::Active,
            reason: Reason::Manual,
            evidence: String::new(),
            case_message: None,
            created_at: 0,
            kicked: false,
        };
        app.db
            .call(move |c| crate::db::quarantine::insert_new(c, &record))
            .await
            .unwrap();
        let other = "501".to_owned();
        let banner = apply_role(
            &app,
            guild,
            invoker,
            RoleKind::Quarantine,
            Some(&other),
        )
        .await
        .unwrap();
        assert!(banner.contains("เคสกักกันค้าง 1 เคส"), "{banner}");
        assert!(
            apply_role(&app, guild, invoker, RoleKind::Quarantine, None)
                .await
                .unwrap()
                .contains("ค้าง")
        );
        assert_eq!(
            app.guild(guild)
                .role_id(RoleKind::Quarantine),
            Some(current)
        );
        let same = "500".to_owned();
        let banner = apply_role(
            &app,
            guild,
            invoker,
            RoleKind::Quarantine,
            Some(&same),
        )
        .await
        .unwrap();
        assert!(banner.contains("ตั้งค่าแล้ว"), "{banner}");
    }
}
