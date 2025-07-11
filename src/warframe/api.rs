#[cfg(test)]
use once_cell::sync::OnceCell;
use serde::Deserialize;

use crate::services::http::HttpService;

const BASE_URL: &str = "https://api.warframestat.us/pc";

#[cfg(test)]
static BASE_URL_OVERRIDE: OnceCell<String> = OnceCell::new();

#[derive(Deserialize)]
pub struct NewsItem {
    #[serde(rename = "imageLink")]
    pub image_link: Option<String>,
}

#[derive(Deserialize)]
pub struct Cycle {
    pub state: String,
    pub expiry: String,
}

#[derive(Deserialize)]
pub struct SteelPathReward {
    pub name: String,
}

#[derive(Deserialize)]
pub struct SteelPathData {
    #[serde(rename = "currentReward")]
    pub current_reward: Option<SteelPathReward>,
    pub expiry: String,
    pub activation: Option<String>,
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(path: &str) -> anyhow::Result<T> {
    let base = {
        #[cfg(test)]
        {
            if let Some(url) = BASE_URL_OVERRIDE.get() {
                url.as_str()
            } else {
                BASE_URL
            }
        }
        #[cfg(not(test))]
        {
            BASE_URL
        }
    };
    let url = format!("{base}/{path}");
    Ok(HttpService::get(url).await?.json::<T>().await?)
}

pub async fn news() -> anyhow::Result<Vec<NewsItem>> {
    fetch_json("news").await
}

pub async fn cycle(endpoint: &str) -> anyhow::Result<Cycle> {
    fetch_json(endpoint).await
}

pub async fn steel_path() -> anyhow::Result<SteelPathData> {
    fetch_json("steelPath").await
}

#[cfg(test)]
pub fn set_base_url(url: &str) {
    let _ = BASE_URL_OVERRIDE.set(url.to_string());
}
