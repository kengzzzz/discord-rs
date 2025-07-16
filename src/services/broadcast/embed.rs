use chrono::Utc;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::{Message, message::Embed},
    id::{Id, marker::GuildMarker},
    util::Timestamp,
};
use twilight_util::builder::embed::{EmbedAuthorBuilder, EmbedBuilder, ImageSource};

use crate::utils::embed::footer_with_icon;

use super::BroadcastService;

const COLOR: u32 = 0x6495ED;
impl BroadcastService {
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
                .color(COLOR)
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
                    builder =
                        builder.description(format!("{}\n{}", message.content, attachment.url));
                }
            } else {
                builder = builder.description(format!("{}\n{}", message.content, attachment.url));
            }

            embeds.push(builder.build());
        }

        if embeds.is_empty() {
            let embed = EmbedBuilder::new()
                .color(COLOR)
                .author(author)
                .description(&message.content)
                .footer(footer)
                .timestamp(Timestamp::from_micros(Utc::now().timestamp_micros())?)
                .build();
            embeds.push(embed);
        }

        Ok::<_, anyhow::Error>(embeds)
    }
}
