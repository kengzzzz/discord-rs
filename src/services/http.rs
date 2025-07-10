use once_cell::sync::Lazy;
use reqwest::{Client, Error, IntoUrl, RequestBuilder, Response};
use std::sync::Arc;

static HTTP_CLIENT: Lazy<Arc<Client>> = Lazy::new(|| {
    let client = Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("Failed to build Client");
    Arc::new(client)
});

pub struct HttpService;

impl HttpService {
    pub async fn get(url: impl IntoUrl) -> Result<Response, Error> {
        HTTP_CLIENT.get(url).send().await?.error_for_status()
    }

    pub fn post(url: impl IntoUrl) -> RequestBuilder {
        HTTP_CLIENT.post(url)
    }

    pub fn delete(url: impl IntoUrl) -> RequestBuilder {
        HTTP_CLIENT.delete(url)
    }

    pub fn patch(url: impl IntoUrl) -> RequestBuilder {
        HTTP_CLIENT.patch(url)
    }

    pub fn put(url: impl IntoUrl) -> RequestBuilder {
        HTTP_CLIENT.put(url)
    }
}
