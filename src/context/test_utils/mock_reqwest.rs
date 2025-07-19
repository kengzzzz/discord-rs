use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::http::HeaderMap;
use reqwest::{Body, Client, Response, Url};

use crate::services::ai::attachments::AttachmentHttp;
use crate::utils::http::HttpProvider;

#[derive(Clone, Default)]
pub struct MockReqwest {
    responses: Arc<Mutex<HashMap<String, String>>>,
    client: Client,
}

impl MockReqwest {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            client: Client::new(),
        }
    }

    pub fn add_json_response(&self, url: &str, body: &str) {
        self.responses
            .lock()
            .unwrap()
            .insert(url.to_string(), body.to_string());
    }
}

#[async_trait]
impl HttpProvider for MockReqwest {
    async fn get_json<T>(&self, url: &str) -> anyhow::Result<T>
    where
        T: serde::de::DeserializeOwned + Send,
    {
        let map = self.responses.lock().unwrap();
        let body = map.get(url).cloned().unwrap_or_else(|| "null".to_string());
        Ok(serde_json::from_str(&body)?)
    }

    fn as_reqwest(&self) -> &Client {
        &self.client
    }
}

#[async_trait]
impl AttachmentHttp for MockReqwest {
    async fn get(&self, _url: &str) -> reqwest::Result<Response> {
        unimplemented!("MockReqwest::get")
    }

    async fn post(&self, _url: Url, _headers: HeaderMap, _body: Body) -> reqwest::Result<Response> {
        unimplemented!("MockReqwest::post")
    }
}
