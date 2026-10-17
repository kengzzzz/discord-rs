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

#[tokio::test]
async fn circuit_fetches_steel_path_choices_and_weekly_dates() {
    // Trimmed from DE's public worldstate on 2026-10-17.
    let body = include_str!("fixtures/circuit.json");
    let client = MockHttp::new(HashMap::from([(
        CIRCUIT_URL.to_owned(),
        body.to_owned(),
    )]));
    let now = "2026-10-17T14:11:48Z".parse().unwrap();

    let data = circuit(&client, now).await.unwrap();

    assert_eq!(
        data.activation.to_rfc3339(),
        "2026-10-12T00:00:00+00:00"
    );
    assert_eq!(
        data.expiry.to_rfc3339(),
        "2026-10-19T00:00:00+00:00"
    );
    assert_eq!(
        data.weapons,
        ["Zylok", "Sibear", "Dread", "Despair", "Hate"]
    );
    assert_eq!(data.warframes, ["Nekros", "Valkyr", "Oberon"]);
    assert!(
        circuit(&client, data.activation)
            .await
            .is_ok()
    );
    assert!(
        circuit(&client, data.expiry)
            .await
            .is_err()
    );
    assert!(
        circuit(
            &client,
            data.activation - chrono::Duration::seconds(1)
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn circuit_selects_the_active_entry_instead_of_the_first() {
    let mut body: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/circuit.json")).unwrap();
    let mut future = body["EndlessXpSchedule"][0].clone();
    future["Activation"]["$date"]["$numberLong"] = "1792368000000".into();
    future["Expiry"]["$date"]["$numberLong"] = "1792972800000".into();
    future["CategoryChoices"][1]["Choices"] =
        serde_json::json!(["Dera", "Sybaris", "Cestra", "Sicarus", "Okina"]);
    body["EndlessXpSchedule"]
        .as_array_mut()
        .unwrap()
        .insert(0, future);
    let client = MockHttp::new(HashMap::from([(
        CIRCUIT_URL.to_owned(),
        body.to_string(),
    )]));

    let current = circuit(&client, "2026-10-17T14:11:48Z".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(current.weapons[0], "Zylok");
    let next = circuit(&client, current.expiry)
        .await
        .unwrap();
    assert_eq!(next.weapons[0], "Dera");
}

#[tokio::test]
async fn circuit_tolerates_missing_normal_choices() {
    let mut body: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/circuit.json")).unwrap();
    body["EndlessXpSchedule"][0]["CategoryChoices"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    let client = MockHttp::new(HashMap::from([(
        CIRCUIT_URL.to_owned(),
        body.to_string(),
    )]));

    let data = circuit(&client, "2026-10-17T14:11:48Z".parse().unwrap())
        .await
        .unwrap();
    assert!(data.warframes.is_empty());
    assert_eq!(data.weapons[0], "Zylok");
}

#[test]
fn circuit_cache_entries_without_warframes_still_deserialize() {
    let data: CircuitData = serde_json::from_str(
        r#"{"activation":"2026-10-12T00:00:00Z","expiry":"2026-10-19T00:00:00Z","weapons":["Zylok"]}"#,
    )
    .unwrap();
    assert!(data.warframes.is_empty());
}

#[tokio::test]
async fn circuit_rejects_missing_or_empty_steel_path_choices() {
    let original: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/circuit.json")).unwrap();
    for missing in [true, false] {
        let mut body = original.clone();
        if missing {
            body["EndlessXpSchedule"][0]["CategoryChoices"]
                .as_array_mut()
                .unwrap()
                .pop();
        } else {
            body["EndlessXpSchedule"][0]["CategoryChoices"][1]["Choices"] = serde_json::json!([]);
        }
        let client = MockHttp::new(HashMap::from([(
            CIRCUIT_URL.to_owned(),
            body.to_string(),
        )]));
        let error = circuit(&client, "2026-10-17T14:11:48Z".parse().unwrap())
            .await
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("no Steel Path choices")
        );
    }
}
