use chrono::DateTime;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::message::Embed,
    id::{Id, marker::GuildMarker},
};
use twilight_util::builder::embed::{EmbedAuthorBuilder, EmbedBuilder, ImageSource};

use crate::utils::embed::footer_with_icon;

use super::{
    BuildService,
    client::{BuildData, MAX_BUILDS},
};

const COLOR: u32 = 0xF1C40F;
const BASE_URL: &str = "https://overframe.gg";
const ICON_URL: &str = "https://static.overframe.gg/static/images/logos/logo-64.png";

impl BuildService {
    pub(super) fn build_not_found_embed(
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    ) -> anyhow::Result<Embed> {
        let mut footer = footer_with_icon(guild)?;
        footer.text = guild.name().to_string();
        let embed = EmbedBuilder::new()
            .color(COLOR)
            .title("ไม่พบ build")
            .description("กรุณาตรวจสอบชื่อ item อีกครั้ง")
            .footer(footer)
            .build();
        Ok(embed)
    }

    pub(super) fn build_error_embed(
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    ) -> anyhow::Result<Embed> {
        let mut footer = footer_with_icon(guild)?;
        footer.text = guild.name().to_string();
        let embed = EmbedBuilder::new()
            .color(COLOR)
            .title("เกิดข้อผิดพลาด")
            .description("กรุณาลองอีกครั้ง ภายหลัง")
            .footer(footer)
            .build();
        Ok(embed)
    }

    pub(super) fn build_embed(
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
        item: &str,
        build: BuildData,
    ) -> anyhow::Result<Embed> {
        let mut footer = footer_with_icon(guild)?;
        footer.text = guild.name().to_string();

        let date = DateTime::parse_from_rfc3339(&build.updated)
            .ok()
            .map(|dt| dt.format("%-d %B %Y").to_string())
            .unwrap_or_default();

        let author = EmbedAuthorBuilder::new(format!("{} by {}", item, build.author.username))
            .icon_url(ImageSource::url(ICON_URL)?)
            .url(format!("{BASE_URL}{}", build.author.url))
            .build();

        let embed = EmbedBuilder::new()
            .color(COLOR)
            .author(author)
            .title(build.title)
            .description(format!("[ {} Forma ] [ {} ]", build.formas, date))
            .url(format!("{BASE_URL}{}", build.url))
            .footer(footer)
            .build();
        Ok(embed)
    }

    pub(super) fn build_embeds_internal(
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
        item: &str,
        builds: Vec<BuildData>,
    ) -> anyhow::Result<Vec<Embed>> {
        if builds.is_empty() {
            Ok(vec![Self::build_not_found_embed(guild)?])
        } else {
            let mut embeds = Vec::new();
            for b in builds.into_iter().take(MAX_BUILDS) {
                embeds.push(Self::build_embed(guild, item, b)?);
            }
            Ok(embeds)
        }
    }
}
