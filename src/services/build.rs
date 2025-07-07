use std::{
    collections::HashSet,
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use once_cell::sync::Lazy;
use reqwest::Client;
use serde::Deserialize;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::message::Embed,
    id::{Id, marker::GuildMarker},
};
use twilight_util::builder::embed::EmbedBuilder;

use crate::{
    dbs::redis::{redis_get, redis_set},
    utils::embed::footer_with_icon,
};

const ITEMS_URL: &str =
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/All.json";
const COLOR: u32 = 0xF1C40F;
const BASE_URL: &str = "https://overframe.gg";
const API_URL: &str = "https://overframe.gg/api/v1/builds";
const ICON_URL: &str = "https://static.overframe.gg/static/images/logos/logo-64.png";
const UPDATE_SECS: u64 = 60 * 60;
const REDIS_KEY: &str = "discord-bot:build-items";
const MAX_BUILDS: usize = 5;

type ItemEntry = (String, String); // (original, lowercase)
static ITEMS: Lazy<RwLock<Vec<ItemEntry>>> = Lazy::new(|| RwLock::new(Vec::new()));
static LAST_UPDATE: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

#[derive(Deserialize)]
struct Item {
    name: String,
    category: String,
    #[serde(rename = "productCategory")]
    product_category: Option<String>,
}

#[derive(Deserialize)]
struct BuildAuthor {
    username: String,
    url: String,
}

#[derive(Deserialize)]
struct BuildData {
    title: String,
    url: String,
    formas: u32,
    updated: String,
    author: BuildAuthor,
}

#[derive(Deserialize)]
struct BuildList {
    results: Vec<BuildData>,
}

const CATEGORY: [&str; 9] = [
    "Primary",
    "Melee",
    "Secondary",
    "Pets",
    "Arch-Melee",
    "Archwing",
    "Warframes",
    "Sentinels",
    "Arch-Gun",
];

fn filter(item: &Item) -> bool {
    if CATEGORY.contains(&item.category.as_str()) {
        return true;
    }
    if item.category == "Misc" {
        if let Some(pc) = &item.product_category {
            return pc == "Pistols" || pc == "SpecialItems";
        }
    }
    false
}

async fn load_from_redis() -> Option<Vec<ItemEntry>> {
    let redis_key = REDIS_KEY;
    if let Some(names) = redis_get::<Vec<String>>(redis_key).await {
        let entries = names
            .into_iter()
            .map(|n| {
                let lower = n.to_lowercase();
                (n, lower)
            })
            .collect();
        return Some(entries);
    }
    None
}

async fn update_items() -> anyhow::Result<()> {
    let client = Client::new();
    let resp = client.get(ITEMS_URL).send().await?.error_for_status()?;
    let fetched: Vec<Item> = resp.json().await?;
    let mut set = HashSet::new();
    let mut names = Vec::new();
    let mut original = Vec::new();
    for item in fetched {
        if filter(&item) {
            let lower = item.name.to_lowercase();
            if set.insert(lower.clone()) {
                original.push(item.name.clone());
                names.push((item.name, lower));
            }
        }
    }
    names.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    original.sort_unstable();
    *ITEMS.write().expect("ITEMS lock poisoned") = names;
    redis_set(REDIS_KEY, &original).await;
    LAST_UPDATE.store(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        Ordering::Relaxed,
    );
    Ok(())
}

pub struct BuildService;

impl BuildService {
    pub async fn init() {
        if let Some(data) = load_from_redis().await {
            *ITEMS.write().expect("ITEMS lock poisoned") = data;
            LAST_UPDATE.store(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                Ordering::Relaxed,
            );
        } else if let Err(e) = update_items().await {
            tracing::warn!(error = %e, "failed to update build items");
        }
        tokio::spawn(async {
            loop {
                tokio::time::sleep(Duration::from_secs(UPDATE_SECS)).await;
                if let Err(e) = update_items().await {
                    tracing::warn!(error = %e, "failed to update build items");
                }
            }
        });
    }

    pub fn search(prefix: &str) -> Vec<String> {
        let p = prefix.to_lowercase();
        let items = ITEMS.read().expect("ITEMS lock poisoned");
        items
            .iter()
            .filter(|(_, lower)| lower.starts_with(&p))
            .take(25)
            .map(|(orig, _)| orig.clone())
            .collect()
    }

    async fn maybe_refresh() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_UPDATE.load(Ordering::Relaxed);
        if now.saturating_sub(last) > UPDATE_SECS {
            if let Err(e) = update_items().await {
                tracing::warn!(error = %e, "failed to update build items");
            }
        }
    }

    pub async fn search_with_update(prefix: &str) -> Vec<String> {
        let mut results = Self::search(prefix);
        if results.is_empty() {
            Self::maybe_refresh().await;
            results = Self::search(prefix);
        }
        results
    }

    fn sanitize_item_name(s: &str) -> String {
        s.to_lowercase().replace(' ', "-").replace('&', "%26")
    }

    async fn fetch_builds(item: &str) -> anyhow::Result<Vec<BuildData>> {
        let client = Client::new();
        let mut url =
            format!("{API_URL}?item_name={item}&author_id=10027&limit={MAX_BUILDS}&sort_by=Score");
        let resp = client.get(&url).send().await?.error_for_status()?;
        let mut data: BuildList = resp.json().await?;
        if data.results.is_empty() {
            url = format!("{API_URL}?item_name={item}&limit={MAX_BUILDS}&sort_by=Score");
            let resp = client.get(&url).send().await?.error_for_status()?;
            data = resp.json().await?;
        }
        Ok(data.results)
    }

    fn build_not_found_embed(
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

    fn build_error_embed(
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

    fn build_embed(
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
        item: &str,
        build: BuildData,
    ) -> anyhow::Result<Embed> {
        use chrono::DateTime;
        use twilight_util::builder::embed::{EmbedAuthorBuilder, ImageSource};

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

    pub async fn build_embeds(
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
        item: &str,
    ) -> anyhow::Result<Vec<Embed>> {
        let target = Self::sanitize_item_name(item);
        match Self::fetch_builds(&target).await {
            Ok(builds) => {
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
            Err(e) => {
                tracing::warn!(error = %e, "failed to fetch builds");
                Ok(vec![Self::build_error_embed(guild)?])
            }
        }
    }
}
