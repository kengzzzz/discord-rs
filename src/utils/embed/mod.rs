use std::fmt::Display;

use chrono::Utc;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::util::Timestamp;
use twilight_model::{
    channel::{Message, message::Embed},
    id::{Id, marker::GuildMarker},
};
use twilight_util::builder::embed::{
    EmbedAuthorBuilder, EmbedBuilder, EmbedFieldBuilder, ImageSource,
};

pub(super) const COLOR: u32 = 0xF1C40F;
pub(super) const COLOR_INVALID: u32 = 0xE74C3C;

pub mod general;
mod intro;

pub use general::{footer_with_icon, guild_only_embed, pinging_embed, pong_embed};
pub use intro::*;

pub fn welcome_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    member_tag: &str,
    member_name: &str,
) -> anyhow::Result<Embed> {
    let now = Utc::now().timestamp();

    let mut footer = footer_with_icon(guild)?;
    footer.text = format!("{} welcomes you!", guild.name());

    let mut builder = EmbedBuilder::new()
        .color(COLOR)
        .title(format!("สวัสดี {member_tag}, ยินดีต้อนรับ! 👋"))
        .description(format!(
            "ขอบคุณที่แนะนำตัวด้วยชื่อ **{}**!\nตอนนี้คุณเป็นส่วนหนึ่งของ **{}** แล้ว 🎉",
            member_name,
            guild.name(),
        ))
        .field(EmbedFieldBuilder::new("เวลา", format!("<t:{now}:R>")).inline())
        .footer(footer);

    if let Some(icon_hash) = guild.icon() {
        let url = format!(
            "https://cdn.discordapp.com/icons/{}/{}.png",
            guild.id(),
            icon_hash
        );
        builder = builder.thumbnail(ImageSource::url(url)?);
    }

    let embed = builder.validate()?.build();
    Ok(embed)
}

pub fn set_channel_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    channel_name: &str,
    channel_id: impl Display,
    channel_type: &str,
    setter_name: &str,
) -> anyhow::Result<Embed> {
    let now = Utc::now().timestamp();

    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();

    let embed = EmbedBuilder::new()
        .color(COLOR)
        .title(format!("ตั้งค่าห้อง “{channel_name}”"))
        .description("การตั้งค่าสำเร็จ 🎉")
        .field(EmbedFieldBuilder::new("ชื่อห้อง", channel_name))
        .field(EmbedFieldBuilder::new("รหัสห้อง", channel_id.to_string()))
        .field(EmbedFieldBuilder::new("ชนิดห้อง", channel_type))
        .field(EmbedFieldBuilder::new("ผู้ตั้งค่า", setter_name).inline())
        .field(EmbedFieldBuilder::new("ตั้งเมื่อ", format!("<t:{now}:R>")).inline())
        .footer(footer)
        .validate()?;
    Ok(embed.build())
}

pub fn set_role_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    role_name: &str,
    role_id: impl Display,
    role_type: &str,
    setter: &str,
) -> anyhow::Result<Embed> {
    let now = Utc::now().timestamp();

    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();

    let embed = EmbedBuilder::new()
        .color(COLOR)
        .title(format!("ตั้งค่า Role “{role_name}”"))
        .description("การตั้งค่าสำเร็จ 🎉")
        .field(EmbedFieldBuilder::new("ชื่อ Role", role_name))
        .field(EmbedFieldBuilder::new("รหัส Role", role_id.to_string()))
        .field(EmbedFieldBuilder::new("ชนิด Role", role_type))
        .field(EmbedFieldBuilder::new("ผู้ตั้งค่า", setter).inline())
        .field(EmbedFieldBuilder::new("เวลา", format!("<t:{now}:R>")).inline())
        .footer(footer)
        .validate()?;

    Ok(embed.build())
}

pub fn role_message_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    roles: &[(impl Display, impl Display)],
) -> anyhow::Result<Embed> {
    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();

    let mut builder = EmbedBuilder::new()
        .color(COLOR)
        .title("เลือก Role ที่ต้องการ")
        .description("กดอีโมจิเพื่อรับหรือเอา role ออก");

    for (name, emoji) in roles {
        builder =
            builder.field(EmbedFieldBuilder::new(format!("{emoji} {name}"), "\u{200B}").inline());
    }

    let embed = builder.footer(footer).validate()?.build();
    Ok(embed)
}

pub fn help_embed(guild: &Reference<'_, Id<GuildMarker>, CachedGuild>) -> anyhow::Result<Embed> {
    let footer = footer_with_icon(guild)?;
    let description = "Available commands:\n/ping - Show bot latency\n/verify <token> - Verify yourself\n/warframe market <item> - Check market prices\n/warframe build <item> - Lookup builds\n/ai prompt <text> - Set personal prompt\n/ai talk <message> - Chat with AI\n/ai clear - Clear AI history";
    let embed = EmbedBuilder::new()
        .color(COLOR)
        .title("Help")
        .description(description)
        .footer(footer)
        .validate()?
        .build();
    Ok(embed)
}
