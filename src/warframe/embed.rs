use chrono::Utc;
use twilight_cache_inmemory::Reference;
use twilight_cache_inmemory::model::CachedGuild;
use twilight_model::channel::message::{Embed, embed::EmbedField};
use twilight_model::id::{Id, marker::GuildMarker};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, ImageSource};

use std::sync::Arc;

use super::api;
use super::utils::{format_time, title_case};
use crate::configs::Reaction;
use crate::context::Context;
use crate::utils::embed::footer_with_icon;

const COLOR: u32 = 0xF1C40F;
const URL: &str = "https://github.com/kengzzzz/discord-rs";

async fn image_link(ctx: Arc<Context>) -> anyhow::Result<Option<String>> {
    match api::news(ctx.reqwest.as_ref()).await {
        Ok(data) => Ok(data.last().and_then(|i| i.image_link.clone())),
        Err(e) => {
            tracing::warn!(error = %e, "failed to fetch news image");
            Ok(None)
        }
    }
}

async fn cycle_field(ctx: Arc<Context>, endpoint: &str, name: &str) -> anyhow::Result<EmbedField> {
    let data = api::cycle(ctx.reqwest.as_ref(), endpoint).await?;
    let field = EmbedFieldBuilder::new(
        format!(
            "{}{}{}",
            Reaction::Load.emoji(),
            name,
            Reaction::Load.emoji()
        ),
        format!("{}\n{}", title_case(&data.state), format_time(&data.expiry)),
    )
    .inline()
    .build();
    Ok(field)
}

pub async fn steel_path_field(ctx: Arc<Context>) -> anyhow::Result<(EmbedField, bool)> {
    let data = api::steel_path(ctx.reqwest.as_ref()).await?;
    let mut is_umbra = false;
    if let Some(reward) = &data.current_reward {
        if reward.name == "Umbra Forma Blueprint" {
            if let Some(act) = &data.activation {
                if let Ok(t) = chrono::DateTime::parse_from_rfc3339(act) {
                    let diff = (chrono::Utc::now() - t.with_timezone(&chrono::Utc))
                        .num_minutes()
                        .abs();
                    is_umbra = diff <= 5;
                }
            }
        } else {
            is_umbra = false;
        }
    }
    let value = format!(
        "**{}**\nends {}",
        data.current_reward.map(|r| r.name).unwrap_or_default(),
        format_time(&data.expiry)
    );
    let field = EmbedFieldBuilder::new(
        format!(
            "{}Steel Path{}",
            Reaction::Load.emoji(),
            Reaction::Load.emoji()
        ),
        value,
    )
    .inline()
    .build();
    Ok((field, is_umbra))
}

pub async fn status_embed(
    ctx: Arc<Context>,
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
) -> anyhow::Result<(Embed, bool)> {
    let image_fut = image_link(ctx.clone());
    let steel_fut = steel_path_field(ctx.clone());
    let earth_fut = cycle_field(ctx.clone(), "earthCycle", "Earth");
    let cetus_fut = cycle_field(ctx.clone(), "cetusCycle", "Cetus");
    let vallis_fut = cycle_field(ctx.clone(), "vallisCycle", "Vallis");
    let cambion_fut = cycle_field(ctx.clone(), "cambionCycle", "Cambion");
    let zariman_fut = cycle_field(ctx.clone(), "zarimanCycle", "Zariman");

    let (image, (steel, is_umbra), earth, cetus, vallis, cambion, zariman) = tokio::try_join!(
        image_fut,
        steel_fut,
        earth_fut,
        cetus_fut,
        vallis_fut,
        cambion_fut,
        zariman_fut
    )?;

    let mut builder = EmbedBuilder::new()
        .title("[PC] Warframe Cycle Timers")
        .url(URL)
        .color(COLOR)
        .field(steel)
        .field(earth)
        .field(cetus)
        .field(vallis)
        .field(cambion)
        .field(zariman)
        .timestamp(twilight_model::util::Timestamp::from_micros(
            Utc::now().timestamp_micros(),
        )?);

    if let Some(img) = image {
        if let Ok(img_src) = ImageSource::url(&img) {
            builder = builder.image(img_src);
        }
    }

    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();

    let embed = builder.footer(footer).build();
    Ok((embed, is_umbra))
}
