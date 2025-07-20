use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::channel::message::{Embed, embed::EmbedFooter};
use twilight_model::id::{Id, marker::GuildMarker};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder, ImageSource};

use super::{COLOR, COLOR_INVALID};

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

pub fn guild_only_embed() -> anyhow::Result<Embed> {
    let embed = EmbedBuilder::new()
        .color(COLOR_INVALID)
        .title("This command can only be used in a server")
        .validate()?
        .build();
    Ok(embed)
}

pub fn pong_embed(latency_ms: Option<u64>) -> anyhow::Result<Embed> {
    let desc = match latency_ms {
        Some(ms) => format!("Latency: {ms}ms"),
        None => "Latency: N/A".to_string(),
    };
    let embed = EmbedBuilder::new()
        .color(COLOR)
        .title("Pong!")
        .description(desc)
        .validate()?
        .build();
    Ok(embed)
}

#[cfg(test)]
#[path = "tests/general.rs"]
mod tests;
