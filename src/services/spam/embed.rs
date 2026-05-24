use std::fmt::Display;

use chrono::Utc;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::{Message, message::Embed},
    id::{Id, marker::GuildMarker},
    util::Timestamp,
};
use twilight_util::builder::embed::{
    EmbedAuthorBuilder, EmbedBuilder, EmbedFieldBuilder, ImageSource,
};

use crate::utils::embed::footer_with_icon;

const COLOR_INVALID: u32 = 0xE74C3C;

pub fn quarantine_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    message: &Message,
    channel_id: u64,
    token: &str,
) -> anyhow::Result<Embed> {
    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();

    let mut builder = EmbedAuthorBuilder::new(&message.author.name);
    if let Some(avatar) = &message.author.avatar {
        let url = format!(
            "https://cdn.discordapp.com/avatars/{}/{}.png",
            message.author.id.get(),
            avatar,
        );
        builder = builder.icon_url(ImageSource::url(url)?);
    }
    let author = builder.build();

    let attachment = message.attachments.first();
    let title = if attachment.is_some() {
        "ตรวจพบไฟล์แนบต้องสงสัย"
    } else {
        "ตรวจพบข้อความต้องสงสัย"
    };
    let reason = if attachment.is_some() {
        "ไฟล์แนบรูปภาพ"
    } else {
        "ข้อความตรงกับรูปแบบสแปม"
    };
    let preview = message_preview(message);

    let mut builder = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title(title)
        .description("ระบบป้องกันสแปมกักกันผู้ใช้นี้ไว้ชั่วคราวและต้องยืนยันตัวตน")
        .author(author)
        .field(EmbedFieldBuilder::new("ผู้ใช้", format!("<@{}>", message.author.id)).inline())
        .field(EmbedFieldBuilder::new("ช่องทาง", format!("<#{}>", message.channel_id)).inline())
        .field(EmbedFieldBuilder::new("เหตุผล", reason).inline())
        .field(
            EmbedFieldBuilder::new(
                "ต้องดำเนินการ",
                format!("ใช้คำสั่ง `/verify` ใน <#{channel_id}>"),
            )
            .inline(),
        )
        .field(EmbedFieldBuilder::new("Token สำหรับยืนยัน", format!("```{token}```")).inline())
        .field(EmbedFieldBuilder::new("ตัวอย่างข้อความ", preview))
        .footer(footer)
        .timestamp(Timestamp::from_micros(
            Utc::now().timestamp_micros(),
        )?);

    if let Some(attachment) = attachment {
        builder = builder
            .field(EmbedFieldBuilder::new("ไฟล์แนบ", attachment_summary(attachment)).inline());
    }

    Ok(builder.validate()?.build())
}

pub fn quarantine_reminder_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    channel_id: impl Display,
    token: &str,
) -> anyhow::Result<Embed> {
    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();

    let embed = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title("ต้องทำการยืนยันตัวตน")
        .description(format!(
            "คุณถูกกักกันชั่วคราว ใช้คำสั่ง `/verify` ในช่อง <#{channel_id}> แล้วกรอก token `{token}` เพื่อกลับมาสนทนาอีกครั้ง"
        ))
        .footer(footer)
        .validate()?;

    Ok(embed.build())
}

fn message_preview(message: &Message) -> String {
    let content = message.content.trim();
    if content.is_empty() {
        return "ไม่มีข้อความ (มีไฟล์แนบ)".to_string();
    }

    truncate_field(content)
}

fn attachment_summary(attachment: &twilight_model::channel::Attachment) -> String {
    let size = format_bytes(attachment.size);
    let dimensions = match (attachment.width, attachment.height) {
        (Some(width), Some(height)) => format!("{width}x{height}px"),
        _ => "ไม่ทราบขนาดภาพ".to_string(),
    };

    format!(
        "`{}`\n{}\n{}",
        attachment.filename, size, dimensions
    )
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn truncate_field(value: &str) -> String {
    const MAX_FIELD_CHARS: usize = 900;
    if value.chars().count() <= MAX_FIELD_CHARS {
        return value.to_string();
    }

    let mut truncated: String = value
        .chars()
        .take(MAX_FIELD_CHARS - 1)
        .collect();
    truncated.push('…');
    truncated
}
