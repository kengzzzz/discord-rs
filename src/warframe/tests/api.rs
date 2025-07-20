use super::*;
use async_trait::async_trait;
use std::collections::HashMap;

struct MockHttp {
    data: HashMap<String, String>,
    client: reqwest::Client,
}

impl MockHttp {
    fn new(map: HashMap<String, String>) -> Self {
        Self { data: map, client: reqwest::Client::new() }
    }
}

#[async_trait]
impl HttpProvider for MockHttp {
    async fn get_json<T>(&self, url: &str) -> anyhow::Result<T>
    where
        T: DeserializeOwned + Send,
    {
        let body = self
            .data
            .get(url)
            .expect("missing mock response");
        Ok(serde_json::from_str(body)?)
    }

    fn as_reqwest(&self) -> &reqwest::Client {
        &self.client
    }
}

#[tokio::test]
async fn test_news_fetch_json() {
    let url = format!("{BASE_URL}/news");
    let mut map = HashMap::new();
    map.insert(url, "[{\"imageLink\":\"link.png\"}]".to_string());
    let client = MockHttp::new(map);

    let items = news(&client).await.expect("news call");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].image_link.as_deref(), Some("link.png"));
}

#[tokio::test]
async fn test_cycle_fetch_json() {
    let url = format!("{BASE_URL}/earthCycle");
    let mut map = HashMap::new();
    map.insert(
        url,
        "{\"state\":\"day\",\"expiry\":\"2030-01-01T00:00:00Z\"}".to_string(),
    );
    let client = MockHttp::new(map);

    let cycle = cycle(&client, "earthCycle")
        .await
        .expect("cycle call");
    assert_eq!(cycle.state, "day");
    assert_eq!(cycle.expiry, "2030-01-01T00:00:00Z");
}

#[tokio::test]
async fn test_steel_path_fetch_json() {
    let url = format!("{BASE_URL}/steelPath");
    let mut map = HashMap::new();
    map.insert(url, "{\"currentReward\":{\"name\":\"Forma\"},\"expiry\":\"2030-01-01T00:00:00Z\",\"activation\":\"2029-12-01T00:00:00Z\"}".to_string());
    let client = MockHttp::new(map);

    let sp = steel_path(&client)
        .await
        .expect("steel path");
    assert_eq!(sp.current_reward.unwrap().name, "Forma");
    assert_eq!(sp.expiry, "2030-01-01T00:00:00Z");
    assert_eq!(sp.activation.unwrap(), "2029-12-01T00:00:00Z");
}
