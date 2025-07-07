use super::*;
use crate::services::introduction::IntroDetails;

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

    let embed = EmbedBuilder::new()
        .color(COLOR)
        .title(format!("{member_tag} ได้แนะนำตัวแล้ว"))
        .field(EmbedFieldBuilder::new("ชื่อ", &details.name))
        .field(EmbedFieldBuilder::new("อายุ", age))
        .field(EmbedFieldBuilder::new("IGN", ign))
        .field(EmbedFieldBuilder::new("Clan", clan))
        .footer(footer)
        .timestamp(Timestamp::from_micros(now)?)
        .validate()?
        .build();

    Ok(embed)
}

pub fn intro_success_embed(
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
) -> anyhow::Result<Embed> {
    let mut builder = EmbedBuilder::new().color(COLOR).title("✅ แนะนำตัวสำเร็จ");

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
        .description(format!("เพื่อปลดล็อค กรุณาใช้คำสั่ง verify ใน <#{channel_id}>"))
        .field(EmbedFieldBuilder::new("🔑 token สำหรับ verify", format!("```{token}```")).inline())
        .field(EmbedFieldBuilder::new("💬 ตัวอย่างคำสั่ง", format!("`/verify {token}`")).inline())
        .footer(footer.clone())
        .timestamp(Timestamp::from_micros(Utc::now().timestamp_micros())?)
        .validate()?
        .build();

    let mut builder = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title(&message.content)
        .description("ตัวอย่างข้อความต้องสงสัย")
        .author(author)
        .footer(footer);

    if let Some(attachment) = message.attachments.first() {
        if let Ok(image_source) = ImageSource::url(&attachment.url) {
            builder = builder.image(image_source);
        }
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
            "คุณถูกกักกัน ใช้คำสั่ง `/verify {token}` ในช่อง <#{channel_id}> เพื่อสนทนาอีกครั้ง"
        ))
        .footer(footer)
        .validate()?;

    Ok(embed.build())
}

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
