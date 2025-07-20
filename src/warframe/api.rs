use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::utils::http::HttpProvider;

const BASE_URL: &str = "https://api.warframestat.us/pc";

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
