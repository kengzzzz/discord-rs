use serde::{Deserialize, Serialize};

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

async fn fetch_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    path: &str,
) -> anyhow::Result<T> {
    let base = BASE_URL;
    let url = format!("{base}/{path}");
    let result = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json::<T>()
        .await?;
    Ok(result)
}

pub async fn news(client: &reqwest::Client) -> anyhow::Result<Vec<NewsItem>> {
    fetch_json(client, "news").await
}

pub async fn cycle(client: &reqwest::Client, endpoint: &str) -> anyhow::Result<Cycle> {
    fetch_json(client, endpoint).await
}

pub async fn steel_path(client: &reqwest::Client) -> anyhow::Result<SteelPathData> {
    fetch_json(client, "steelPath").await
}
