use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::message::Embed,
    id::{Id, marker::GuildMarker},
};
use twilight_util::builder::embed::EmbedBuilder;

use crate::utils::embed::footer_with_icon;

const COLOR: u32 = 0xF1C40F;
const COLOR_INVALID: u32 = 0xE74C3C;

pub fn verify_success_embed(guild: &Reference<'_, Id<GuildMarker>, CachedGuild>) -> Option<Embed> {
    let mut footer = footer_with_icon(guild).ok()?;
    footer.text = guild.name().to_string();

    let embed = EmbedBuilder::new()
        .color(COLOR)
        .title("✅ ยืนยันตัวตนสำเร็จ")
        .description("คุณสามารถสนทนาได้แล้ว")
        .footer(footer)
        .validate()
        .ok()?
        .build();

    Some(embed)
}

pub fn verify_fail_embed(guild: &Reference<'_, Id<GuildMarker>, CachedGuild>) -> Option<Embed> {
    let mut footer = footer_with_icon(guild).ok()?;
    footer.text = guild.name().to_string();

    let embed = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title("❌ ยืนยันตัวตนไม่สำเร็จ")
        .description("token ไม่ถูกต้อง กรุณาลองใหม่อีกครั้ง")
        .footer(footer)
        .validate()
        .ok()?
        .build();

    Some(embed)
}

pub fn verify_no_token_embed(guild: &Reference<'_, Id<GuildMarker>, CachedGuild>) -> Option<Embed> {
    let mut footer = footer_with_icon(guild).ok()?;
    footer.text = guild.name().to_string();

    let embed = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title("ไม่มี token สำหรับยืนยันตัวตน")
        .description("คุณไม่ได้อยู่ในสถานะต้องยืนยันตัวตน")
        .footer(footer)
        .validate()
        .ok()?
        .build();

    Some(embed)
}
