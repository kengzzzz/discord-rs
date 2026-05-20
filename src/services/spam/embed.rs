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
) -> anyhow::Result<[Embed; 2]> {
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

    let alert = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title("🚨 ตรวจพบกิจกรรมต้องสงสัย")
        .description(format!(
            "เพื่อปลดล็อค กรุณาใช้คำสั่ง /verify ใน <#{channel_id}>"
        ))
        .field(EmbedFieldBuilder::new("🔑 token สำหรับ verify", format!("```{token}```")).inline())
        .field(EmbedFieldBuilder::new("💬 วิธีการยืนยัน", "กรอก token ในหน้าต่าง /verify").inline())
        .footer(footer.clone())
        .timestamp(Timestamp::from_micros(
            Utc::now().timestamp_micros(),
        )?)
        .validate()?
        .build();

    let sample_title =
        if message.content.trim().is_empty() { "Image attachment" } else { &message.content };

    let mut builder = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title(sample_title)
        .description("ตัวอย่างข้อความต้องสงสัย")
        .author(author)
        .footer(footer);

    if let Some(attachment) = message.attachments.first()
        && let Ok(image_source) = ImageSource::url(&attachment.url)
    {
        builder = builder.image(image_source);
    }

    let info = builder.build();

    Ok([alert, info])
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
        .title("⏳ ต้องทำการยืนยันตัวตน")
        .description(format!(
            "คุณถูกกักกัน ใช้คำสั่ง `/verify` ในช่อง <#{channel_id}> แล้วกรอก token `{token}` เพื่อสนทนาอีกครั้ง"
        ))
        .footer(footer)
        .validate()?;

    Ok(embed.build())
}
