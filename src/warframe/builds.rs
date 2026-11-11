use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use serde::Deserialize;
use twilight_model::{application::interaction::Interaction, channel::message::Component};

use super::market::NameIndex;
use crate::{
    app::App,
    db::{self, catalog},
    discord::{
        reply,
        ui::{self, Card, color},
    },
};

const CATALOG: &str = "build_items";
const OVERFRAME: &str = "https://overframe.gg";
const API: &str = "https://overframe.gg/api/v1/builds/";
const THAI_AUTHOR: u32 = 10027;
const LIMIT: usize = 5;

#[derive(Default)]
pub struct Builds {
    names: RwLock<Arc<NameIndex>>,
    item_ids: Mutex<HashMap<String, u64>>,
}

impl Builds {
    pub fn names(&self) -> Arc<NameIndex> {
        Arc::clone(
            &self
                .names
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    pub fn complete(&self, query: &str) -> Vec<String> {
        self.names().search(query, 25)
    }

    fn set(&self, names: Vec<String>) {
        *self
            .names
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::new(NameIndex::new(names));
    }
}

#[derive(Deserialize)]
struct WarframeEntry {
    name: String,
    #[serde(default, rename = "productCategory")]
    product_category: Option<String>,
}

#[derive(Deserialize)]
struct WeaponEntry {
    name: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default, rename = "uniqueName")]
    unique_name: Option<String>,
}

#[derive(Deserialize)]
struct TypedEntry {
    name: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default, rename = "productCategory")]
    product_category: Option<String>,
    #[serde(default, rename = "uniqueName")]
    unique_name: Option<String>,
}

const SOURCES: [&str; 5] = [
    "https://api.warframestat.us/warframes/?only=name,type,productCategory&language=en",
    "https://api.warframestat.us/weapons/?only=name,type,uniqueName&language=en",
    "https://api.warframestat.us/items/search/Sentinels,Pets?by=category&only=name,type&language=en",
    "https://api.warframestat.us/items/search/SentinelWeapons,SpecialItems?by=productCategory&only=name,type,productCategory&language=en",
    "https://api.warframestat.us/items/search/Kitgun%20Component?by=type&only=name,uniqueName,type&language=en",
];

pub fn filter_sources(bodies: &[Vec<u8>; 5]) -> anyhow::Result<Vec<String>> {
    let mut names = Vec::new();
    let frames: Vec<WarframeEntry> = serde_json::from_slice(&bodies[0])?;
    names.extend(
        frames
            .into_iter()
            .filter(|f| {
                matches!(
                    f.product_category.as_deref(),
                    Some("Suits" | "SpaceSuits" | "MechSuits" | "SpecialItems")
                )
            })
            .map(|f| f.name),
    );
    let weapons: Vec<WeaponEntry> = serde_json::from_slice(&bodies[1])?;
    names.extend(
        weapons
            .into_iter()
            .filter(|w| {
                w.kind.as_deref() != Some("Zaw Component")
                    || w.unique_name
                        .as_deref()
                        .and_then(|u| u.rsplit('/').next())
                        .is_some_and(|last| last.contains("Tip"))
            })
            .map(|w| w.name),
    );
    let companions: Vec<TypedEntry> = serde_json::from_slice(&bodies[2])?;
    names.extend(
        companions
            .into_iter()
            .filter(|c| matches!(c.kind.as_deref(), Some("Sentinel" | "Pets")))
            .map(|c| c.name),
    );
    let special: Vec<TypedEntry> = serde_json::from_slice(&bodies[3])?;
    names.extend(
        special
            .into_iter()
            .filter(|s| {
                s.product_category.as_deref() == Some("SentinelWeapons")
                    || (s.product_category.as_deref() == Some("SpecialItems")
                        && s.kind.as_deref() == Some("Exalted Weapon"))
            })
            .map(|s| s.name),
    );
    let kitguns: Vec<TypedEntry> = serde_json::from_slice(&bodies[4])?;
    names.extend(
        kitguns
            .into_iter()
            .filter(|k| {
                k.unique_name
                    .as_deref()
                    .is_some_and(|u| u.contains("/Barrel/"))
            })
            .map(|k| k.name),
    );
    names.retain(|n| !n.trim().is_empty());
    names.sort_by_cached_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    Ok(names)
}

pub async fn load(app: &App) {
    if let Ok(Some(stored)) = app
        .db
        .call(|c| catalog::get(c, CATALOG))
        .await
        && let Ok(names) = serde_json::from_slice::<Vec<String>>(&stored.body)
    {
        app.warframe.builds.set(names);
    }
}

pub async fn refresh(app: &App) -> anyhow::Result<()> {
    let mut bodies: [Vec<u8>; 5] = Default::default();
    for (body, url) in bodies.iter_mut().zip(SOURCES) {
        *body = app
            .web
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec();
    }
    let names = filter_sources(&bodies)?;
    if names.len() < 200 {
        anyhow::bail!(
            "build catalog looks truncated ({} names)",
            names.len()
        );
    }
    let stored =
        catalog::Stored { etag: None, fetched_at: db::now(), body: serde_json::to_vec(&names)? };
    app.db
        .call(move |c| catalog::put(c, CATALOG, &stored))
        .await?;
    tracing::info!(count = names.len(), "build catalog updated");
    app.warframe.builds.set(names);
    Ok(())
}

#[derive(Deserialize, Clone, Debug)]
pub struct Build {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub formas: u32,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub score: i64,
    pub author: Author,
    #[serde(default)]
    pub item_data: Option<ItemData>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Author {
    pub username: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ItemData {
    pub id: u64,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    count: u64,
    #[serde(default)]
    results: Vec<Build>,
}

pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut dash = false;
    for ch in name
        .chars()
        .flat_map(char::to_lowercase)
    {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            dash = false;
        } else if (ch == ' ' || ch == '-') && !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn url_slug(url: &str) -> Option<&str> {
    url.trim_matches('/').split('/').nth(2)
}

async fn query(app: &App, params: &[(&str, String)]) -> anyhow::Result<Page> {
    let _lookup = super::lookup(app).await?;
    let body = super::read_limited(app.web.get(API).query(params)).await?;
    Ok(serde_json::from_slice(&body)?)
}

/// Cached by slug, not by the typed name, so members can't fill the cache.
async fn resolve_item_id(app: &App, name: &str) -> anyhow::Result<Option<u64>> {
    let slug = slugify(name);
    if let Some(id) = app
        .warframe
        .builds
        .item_ids
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&slug)
    {
        return Ok(Some(*id));
    }
    let page = query(
        app,
        &[("title", name.to_owned()), ("limit", "20".into())],
    )
    .await?;
    let id = page
        .results
        .iter()
        .find(|b| url_slug(&b.url) == Some(slug.as_str()))
        .and_then(|b| b.item_data.as_ref().map(|d| d.id));
    if let Some(id) = id {
        app.warframe
            .builds
            .item_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(slug, id);
    }
    Ok(id)
}

pub async fn fetch(app: &App, name: &str) -> anyhow::Result<Vec<Build>> {
    let by_name = |author: Option<u32>| {
        let mut params = vec![
            ("item_name", name.to_owned()),
            ("limit", LIMIT.to_string()),
            ("sort_by", "Score".to_owned()),
        ];
        if let Some(author) = author {
            params.push(("author_id", author.to_string()));
        }
        params
    };
    for author in [Some(THAI_AUTHOR), None] {
        let page = query(app, &by_name(author)).await?;
        if page.count > 0 && page.count < 100_000 && !page.results.is_empty() {
            return Ok(page.results);
        }
    }
    let Some(id) = resolve_item_id(app, name).await? else {
        return Ok(Vec::new());
    };
    for author in [Some(THAI_AUTHOR), None] {
        let mut params = vec![
            ("item_id", id.to_string()),
            ("limit", LIMIT.to_string()),
            ("sort_by", "Score".to_owned()),
        ];
        if let Some(author) = author {
            params.push(("author_id", author.to_string()));
        }
        let page = query(app, &params).await?;
        if !page.results.is_empty() {
            return Ok(page.results);
        }
    }
    Ok(Vec::new())
}

pub fn render(name: &str, builds: &[Build]) -> Vec<Component> {
    let search = reqwest::Url::parse_with_params(&format!("{OVERFRAME}/items/"), &[("q", name)])
        .map(String::from)
        // Link URLs max out at 512 characters, and long Thai queries encode past it.
        .ok()
        .filter(|url| url.len() <= 512)
        .unwrap_or_else(|| format!("{OVERFRAME}/items/"));
    if builds.is_empty() {
        return vec![
            Card::new(color::WARNING)
                .title(format!(
                    "ไม่พบ build ของ {}",
                    ui::escape_markdown(name)
                ))
                .text("เช็คชื่อไอเทม หรือค้นใน overframe.gg")
                .buttons([ui::link(search, "ค้นหาบน overframe.gg")])
                .build(),
        ];
    }
    let mut card = Card::new(color::BRAND).title(format!("Build · {}", ui::escape_markdown(name)));
    for build in builds.iter().take(LIMIT) {
        let date = jiff::Timestamp::strptime("%Y-%m-%dT%H:%M:%S%.f%:z", &build.updated)
            .or_else(|_| build.updated.parse::<jiff::Timestamp>())
            .map(|t| ui::ts(t.as_millisecond(), 'd'))
            .unwrap_or_default();
        let thai = if build
            .author
            .username
            .eq_ignore_ascii_case("UncleCatTH")
        {
            " (ไทย)"
        } else {
            ""
        };
        card = card.section_button(
            format!(
                "**{}**\n-# {} Forma · คะแนน {} · โดย {}{thai} · {date}",
                ui::escape_markdown(&ui::truncate(&build.title, 120)),
                build.formas,
                build.score,
                ui::escape_markdown(&build.author.username)
            ),
            ui::link(format!("{OVERFRAME}{}", build.url), "เปิด"),
        );
    }
    vec![
        card.small("ข้อมูลจาก overframe.gg")
            .build(),
    ]
}

pub async fn command(app: &Arc<App>, interaction: &Interaction, name: &str) -> anyhow::Result<()> {
    reply::defer(app, interaction, true).await?;
    let components = match fetch(app, name.trim()).await {
        Ok(builds) => render(name.trim(), &builds),
        Err(error) => {
            super::log_failure("overframe", &error);
            ui::error("ดึง build จาก overframe.gg ไม่ได้ ลองใหม่ภายหลัง")
        }
    };
    reply::edit(app, interaction, &components).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_card_fits_limits() {
        let build = Build {
            title: "Hate INCARNON | Hybrid slash ".repeat(10),
            url: "/build/494117/hate/hate-incarnon/".into(),
            formas: 5,
            updated: "2026-06-15T12:44:49.549348Z".into(),
            score: 1202,
            author: Author { username: "UncleCatTH".into() },
            item_data: Some(ItemData { id: 1155 }),
        };
        let builds = vec![build; 5];
        let card = render("Hate", &builds);
        crate::discord::ui::validate_message(&card).unwrap();
        assert!(
            serde_json::to_string(&card)
                .unwrap()
                .contains("(ไทย)")
        );
        crate::discord::ui::validate_message(&render("Nothing", &[])).unwrap();
        crate::discord::ui::validate_message(&render(&"ไม่มี".repeat(40), &[])).unwrap();
    }

    #[test]
    fn slugify_matches_overframe_urls() {
        assert_eq!(slugify("Cyte-09"), "cyte-09");
        assert_eq!(slugify("Dark Split-Sword"), "dark-split-sword");
        assert_eq!(slugify("Ack & Brunt"), "ack-brunt");
        assert_eq!(
            url_slug("/build/494117/hate/hate-incarnon/"),
            Some("hate")
        );
    }

    #[test]
    fn source_filters_keep_buildables_only() {
        let bodies = [
            br#"[{"name":"Excalibur","productCategory":"Suits"},{"name":"Helminth","productCategory":null},{"name":"Voidrig","productCategory":"MechSuits"}]"#.to_vec(),
            br#"[{"name":"Hate","type":"Melee","uniqueName":"/a/Hate"},{"name":"Balla","type":"Zaw Component","uniqueName":"/Lotus/Weapons/Ostron/Melee/ModularMelee01/Tip/TipOne"},{"name":"Jai","type":"Zaw Component","uniqueName":"/Lotus/Weapons/Ostron/Melee/ModularMelee01/Handle/HandleOne"},{"name":"hate","type":"Melee"}]"#.to_vec(),
            br#"[{"name":"Smeeta Kavat","type":"Pets"},{"name":"Kavat Collar","type":"Misc"}]"#.to_vec(),
            br#"[{"name":"Exalted Blade","type":"Exalted Weapon","productCategory":"SpecialItems"},{"name":"Deth Machine Rifle","type":"Robotic","productCategory":"SentinelWeapons"}]"#.to_vec(),
            br#"[{"name":"Catchmoon","type":"Kitgun Component","uniqueName":"/x/Barrel/A"},{"name":"Haymaker","type":"Kitgun Component","uniqueName":"/x/Grip/A"}]"#.to_vec(),
        ];
        let names = filter_sources(&bodies).unwrap();
        assert_eq!(
            names,
            [
                "Balla",
                "Catchmoon",
                "Deth Machine Rifle",
                "Exalted Blade",
                "Excalibur",
                "Hate",
                "Smeeta Kavat",
                "Voidrig"
            ]
        );
    }
}
