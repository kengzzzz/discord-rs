use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use twilight_cache_inmemory::Reference;
use twilight_cache_inmemory::model::CachedGuild;
use twilight_model::channel::message::{Embed, embed::EmbedField};
use twilight_model::id::{Id, marker::GuildMarker};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, ImageSource};

use crate::utils::embed::footer_with_icon;

use crate::configs::Reaction;

const BASE_URL: &str = "https://api.warframestat.us/pc";
const COLOR: u32 = 0xF1C40F;
const URL: &str = "https://github.com/kengzzzz/discord-bot";

#[derive(Deserialize)]
struct NewsItem {
    #[serde(rename = "imageLink")]
    image_link: Option<String>,
}

#[derive(Deserialize)]
struct Cycle {
    state: String,
    expiry: String,
}

#[derive(Deserialize)]
struct SteelPathReward {
    name: String,
}

#[derive(Deserialize)]
struct SteelPathData {
    #[serde(rename = "currentReward")]
    current_reward: Option<SteelPathReward>,
    expiry: String,
    activation: Option<String>,
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(
    client: &Client,
    path: &str,
) -> anyhow::Result<T> {
    let url = format!("{BASE_URL}/{path}");
    Ok(client.get(url).send().await?.json::<T>().await?)
}

fn format_time(s: &str) -> String {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        format!("<t:{}:R>", t.timestamp())
    } else {
        String::new()
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::new();
    for (i, part) in s.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = part.chars();
        if let Some(f) = chars.next() {
            out.push_str(&format!(
                "**{}{}",
                f.to_uppercase(),
                chars.as_str().to_lowercase()
            ));
        }
    }
    out.push_str("** ends");
    out
}

async fn image_link(client: &Client) -> anyhow::Result<Option<String>> {
    match fetch_json::<Vec<NewsItem>>(client, "news").await {
        Ok(data) => Ok(data.last().and_then(|i| i.image_link.clone())),
        Err(e) => {
            tracing::warn!(error = %e, "failed to fetch news image");
            Ok(None)
        }
    }
}

async fn cycle_field(client: &Client, endpoint: &str, name: &str) -> anyhow::Result<EmbedField> {
    let data = fetch_json::<Cycle>(client, endpoint).await?;
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

async fn steel_path_field(client: &Client) -> anyhow::Result<(EmbedField, bool)> {
    let data = fetch_json::<SteelPathData>(client, "steelPath").await?;
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
        data.current_reward
            .map(|r| r.name)
            .unwrap_or_else(|| "".to_string()),
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
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
) -> anyhow::Result<(Embed, bool)> {
    let client = Client::new();

    let image_fut = image_link(&client);
    let steel_fut = steel_path_field(&client);
    let earth_fut = cycle_field(&client, "earthCycle", "Earth");
    let cetus_fut = cycle_field(&client, "cetusCycle", "Cetus");
    let vallis_fut = cycle_field(&client, "vallisCycle", "Vallis");
    let cambion_fut = cycle_field(&client, "cambionCycle", "Cambion");
    let zariman_fut = cycle_field(&client, "zarimanCycle", "Zariman");

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
