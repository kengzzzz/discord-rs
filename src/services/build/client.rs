use serde::Deserialize;

use crate::utils::http::HttpProvider;

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

pub(super) async fn fetch_builds<H>(client: &H, item: &str) -> anyhow::Result<Vec<BuildData>>
where
    H: HttpProvider + Sync,
{
    let mut url =
        format!("{API_URL}?item_name={item}&author_id=10027&limit={MAX_BUILDS}&sort_by=Score");
    let mut data: BuildList = client.get_json(&url).await?;
    if data.results.is_empty() {
        url = format!("{API_URL}?item_name={item}&limit={MAX_BUILDS}&sort_by=Score");
        data = client.get_json(&url).await?;
    }
    Ok(data.results)
}
