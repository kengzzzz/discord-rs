use reqwest::Client;
use serde::Deserialize;

pub(super) const MAX_BUILDS: usize = 5;
const API_URL: &str = "https://overframe.gg/api/v1/builds";

#[derive(Deserialize)]
pub(super) struct BuildAuthor {
    pub username: String,
    pub url: String,
}

#[derive(Deserialize)]
pub(super) struct BuildData {
    pub title: String,
    pub url: String,
    pub formas: u32,
    pub updated: String,
    pub author: BuildAuthor,
}

#[derive(Deserialize)]
struct BuildList {
    results: Vec<BuildData>,
}

pub(super) async fn fetch_builds(client: &Client, item: &str) -> anyhow::Result<Vec<BuildData>> {
    let mut url =
        format!("{API_URL}?item_name={item}&author_id=10027&limit={MAX_BUILDS}&sort_by=Score");
    let resp = client.get(url).send().await?.error_for_status()?;
    let mut data: BuildList = resp.json().await?;
    if data.results.is_empty() {
        url = format!("{API_URL}?item_name={item}&limit={MAX_BUILDS}&sort_by=Score");
        let resp = client.get(url).send().await?.error_for_status()?;
        data = resp.json().await?;
    }
    Ok(data.results)
}
