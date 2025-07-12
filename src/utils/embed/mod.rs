use std::fmt::Display;

use chrono::Utc;
use twilight_cache_inmemory::Reference;
use twilight_cache_inmemory::model::CachedGuild;
use twilight_model::channel::Message;
use twilight_model::channel::message::Embed;
use twilight_model::channel::message::embed::EmbedFooter;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;
use twilight_model::util::Timestamp;
use twilight_util::builder::embed::{
    EmbedAuthorBuilder, EmbedBuilder, EmbedFieldBuilder, EmbedFooterBuilder, ImageSource,
};

const COLOR: u32 = 0xF1C40F;
const COLOR_INVALID: u32 = 0xE74C3C;
const COLOR_BROADCAST: u32 = 0x6495ED;
const COLOR_AI: u32 = 0x5865F2;
mod intro;
pub use intro::*;

pub fn footer_with_icon(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
) -> anyhow::Result<EmbedFooter> {
    let mut builder = EmbedFooterBuilder::new(guild.name());
    if let Some(icon) = guild.icon() {
        let url = format!(
            "https://cdn.discordapp.com/icons/{}/{}.png",
            guild.id(),
            icon
        );
        builder = builder.icon_url(ImageSource::url(url)?);
    }
    Ok(builder.build())
}

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

pub fn broadcast_embeds(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    message: &Message,
) -> anyhow::Result<Vec<Embed>> {
    let mut footer = footer_with_icon(guild)?;
    footer.text = format!("{} [Server]", guild.name());

    let mut author_builder = EmbedAuthorBuilder::new(&message.author.name);
    if let Some(avatar) = &message.author.avatar {
        let url = format!(
            "https://cdn.discordapp.com/avatars/{}/{}.png",
            message.author.id.get(),
            avatar
        );
        author_builder = author_builder.icon_url(ImageSource::url(url)?);
    }
    let author = author_builder.build();

    let mut embeds = Vec::new();
    for attachment in &message.attachments {
        let mut builder = EmbedBuilder::new()
            .color(COLOR_BROADCAST)
            .author(author.clone())
            .footer(footer.clone())
            .timestamp(Timestamp::from_micros(Utc::now().timestamp_micros())?);

        if let Some(ct) = &attachment.content_type {
            if ct.starts_with("image") {
                builder = builder.description(&message.content);
                if let Ok(img) = ImageSource::url(&attachment.url) {
                    builder = builder.image(img);
                }
            } else {
                builder = builder.description(format!("{}\n{}", message.content, attachment.url));
            }
        } else {
            builder = builder.description(format!("{}\n{}", message.content, attachment.url));
        }

        embeds.push(builder.build());
    }

    if embeds.is_empty() {
        let embed = EmbedBuilder::new()
            .color(COLOR_BROADCAST)
            .author(author)
            .description(&message.content)
            .footer(footer)
            .timestamp(Timestamp::from_micros(Utc::now().timestamp_micros())?)
            .build();
        embeds.push(embed);
    }

    Ok(embeds)
}

pub fn ai_embeds(text: &str) -> anyhow::Result<Vec<Embed>> {
    const LIMIT: usize = 1024;
    let mut embeds = Vec::new();
    if text.is_empty() {
        return Ok(embeds);
    }

    let mut remaining = text.trim();
    while !remaining.is_empty() {
        let mut end = remaining
            .char_indices()
            .take(LIMIT)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or_else(|| remaining.len());

        if end < remaining.len() {
            if let Some(pos) = remaining[..end].rfind(|c: char| c.is_whitespace()) {
                end = pos + 1;
            }
        }

        let chunk = &remaining[..end];
        let embed = EmbedBuilder::new()
            .color(COLOR_AI)
            .description(chunk)
            .validate()?
            .build();
        embeds.push(embed);
        remaining = remaining[end..].trim_start();
    }
    Ok(embeds)
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

pub fn guild_only_embed() -> anyhow::Result<Embed> {
    let embed = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title("This command can only be used in a server")
        .validate()?
        .build();
    Ok(embed)
}

pub fn pinging_embed() -> anyhow::Result<Embed> {
    let embed = EmbedBuilder::new()
        .color(COLOR)
        .title("Pinging...")
        .validate()?
        .build();
    Ok(embed)
}

pub fn pong_embed(latency_ms: u128) -> anyhow::Result<Embed> {
    let embed = EmbedBuilder::new()
        .color(COLOR)
        .title("Pong!")
        .description(format!("Latency: {latency_ms}ms"))
        .validate()?
        .build();
    Ok(embed)
}
