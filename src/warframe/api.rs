use anyhow::Context as _;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::utils::http::HttpProvider;

const BASE_URL: &str = "https://api.warframestat.us/pc";
pub(super) const CIRCUIT_URL: &str = "https://api.warframe.com/cdn/worldState.php";

#[derive(Serialize, Deserialize)]
pub struct NewsItem {
    #[serde(rename = "imageLink")]
    pub image_link: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Cycle {
    pub state: String,
    pub expiry: String,
}

#[derive(Serialize, Deserialize)]
pub struct SteelPathReward {
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct SteelPathData {
    #[serde(rename = "currentReward")]
    pub current_reward: Option<SteelPathReward>,
    pub expiry: String,
    pub activation: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct CircuitData {
    pub activation: DateTime<Utc>,
    pub expiry: DateTime<Utc>,
    pub weapons: Vec<String>,
    #[serde(default)]
    pub warframes: Vec<String>,
}

#[derive(Deserialize)]
struct CircuitWorldState {
    #[serde(rename = "EndlessXpSchedule")]
    schedules: Vec<CircuitSchedule>,
}

#[derive(Deserialize)]
struct CircuitSchedule {
    #[serde(rename = "Activation", deserialize_with = "worldstate_date")]
    activation: DateTime<Utc>,
    #[serde(rename = "Expiry", deserialize_with = "worldstate_date")]
    expiry: DateTime<Utc>,
    #[serde(rename = "CategoryChoices")]
    categories: Vec<CircuitChoice>,
}

#[derive(Deserialize)]
struct CircuitChoice {
    #[serde(rename = "Category")]
    category: String,
    #[serde(rename = "Choices")]
    choices: Vec<String>,
}

fn worldstate_date<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct Date {
        #[serde(rename = "$date")]
        date: Milliseconds,
    }
    #[derive(Deserialize)]
    struct Milliseconds {
        #[serde(rename = "$numberLong")]
        value: String,
    }

    let date = Date::deserialize(deserializer)?;
    let millis = date
        .date
        .value
        .parse()
        .map_err(serde::de::Error::custom)?;
    DateTime::from_timestamp_millis(millis)
        .ok_or_else(|| serde::de::Error::custom("worldstate date is out of range"))
}

pub async fn circuit<H>(client: &H, now: DateTime<Utc>) -> anyhow::Result<CircuitData>
where
    H: HttpProvider + Sync,
{
    let data: CircuitWorldState = client.get_json(CIRCUIT_URL).await?;
    let schedule = data
        .schedules
        .into_iter()
        .find(|schedule| schedule.activation <= now && now < schedule.expiry)
        .context("no active Circuit schedule in worldstate")?;
    let mut weapons = None;
    let mut warframes = Vec::new();
    for choice in schedule.categories {
        match choice.category.as_str() {
            "EXC_HARD" if !choice.choices.is_empty() => weapons = Some(choice.choices),
            "EXC_NORMAL" => warframes = choice.choices,
            _ => {}
        }
    }
    Ok(CircuitData {
        activation: schedule.activation,
        expiry: schedule.expiry,
        weapons: weapons.context("no Steel Path choices in the active Circuit schedule")?,
        warframes,
    })
}

async fn fetch_json<H, T>(client: &H, path: &str) -> anyhow::Result<T>
where
    H: HttpProvider + Sync,
    T: DeserializeOwned + Send,
{
    let base = BASE_URL;
    let url = format!("{base}/{path}");
    client.get_json(&url).await
}

pub async fn news<H>(client: &H) -> anyhow::Result<Vec<NewsItem>>
where
    H: HttpProvider + Sync,
{
    fetch_json(client, "news").await
}

pub async fn cycle<H>(client: &H, endpoint: &str) -> anyhow::Result<Cycle>
where
    H: HttpProvider + Sync,
{
    fetch_json(client, endpoint).await
}

pub async fn steel_path<H>(client: &H) -> anyhow::Result<SteelPathData>
where
    H: HttpProvider + Sync,
{
    fetch_json(client, "steelPath").await
}

#[cfg(test)]
#[path = "tests/api.rs"]
mod tests;
