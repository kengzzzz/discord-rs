use std::fmt::Display;

use chrono::Utc;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::message::Embed,
    id::{Id, marker::GuildMarker},
    util::Timestamp,
};
use twilight_util::builder::embed::{EmbedBuilder, ImageSource};

use crate::services::introduction::form::IntroDetails;
use crate::utils::embed::footer_with_icon;

const COLOR: u32 = 0xF1C40F;
const COLOR_INVALID: u32 = 0xE74C3C;

pub fn intro_prompt_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    intro_channel_id: impl Display,
) -> anyhow::Result<Embed> {
    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();

    let embed = EmbedBuilder::new()
        .color(COLOR)
        .title(format!("Welcome to {}!", guild.name()))
        .description(format!(
            "กรุณาใช้คำสั่ง `/intro` ใน <#{intro_channel_id}> เพื่อแนะนำตัว\n\n**บอทไม่เก็บข้อมูลที่ใช้ในกระบวนการแนะนำตัว**"
        ))
        .footer(footer)
        .validate()?;

    Ok(embed.build())
}

pub fn intro_unavailable_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
) -> anyhow::Result<Embed> {
    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();

    let embed = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title("ไม่สามารถใช้คำสั่งนี้ได้")
        .description("เซิร์ฟเวอร์นี้ยังไม่ได้ตั้งค่าห้องแนะนำตัว")
        .footer(footer)
        .validate()?;

    Ok(embed.build())
}

pub fn intro_details_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    member_tag: &str,
    details: &IntroDetails,
) -> anyhow::Result<Embed> {
    let now = Utc::now().timestamp_micros();

    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();

    let age = details
        .age
        .map(|a| a.to_string())
        .unwrap_or_else(|| "-".to_string());

    let ign = details.ign.as_deref().unwrap_or("-");
    let clan = details.clan.as_deref().unwrap_or("-");

    let mut builder = EmbedBuilder::new()
        .color(COLOR)
        .title(format!("🚀 {member_tag} ได้แนะนำตัวแล้ว"))
        .description(format!(
            "**ชื่อ:** {}\n**อายุ:** {}\n**IGN:** {}\n**Clan:** {}",
            details.name, age, ign, clan
        ))
        .footer(footer)
        .timestamp(Timestamp::from_micros(now)?);

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

pub fn intro_success_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
) -> anyhow::Result<Embed> {
    let mut builder = EmbedBuilder::new()
        .color(COLOR)
        .title("✅ แนะนำตัวสำเร็จ");

    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();
    builder = builder.footer(footer);

    let embed = builder.validate()?.build();

    Ok(embed)
}

pub fn intro_error_embed() -> anyhow::Result<Embed> {
    let embed = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title("เกิดข้อผิดพลาด")
        .description("ไม่สามารถประมวลผลคำขอได้ กรุณาลองใหม่ภายหลัง")
        .validate()?
        .build();
    Ok(embed)
}
