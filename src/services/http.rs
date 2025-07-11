use reqwest::{Client, Error, IntoUrl, RequestBuilder, Response};

pub struct HttpService;

impl HttpService {
    pub async fn get(client: &Client, url: impl IntoUrl) -> Result<Response, Error> {
        client.get(url).send().await?.error_for_status()
    }

    pub fn post(client: &Client, url: impl IntoUrl) -> RequestBuilder {
        client.post(url)
    }

    pub fn delete(client: &Client, url: impl IntoUrl) -> RequestBuilder {
        client.delete(url)
    }

    pub fn patch(client: &Client, url: impl IntoUrl) -> RequestBuilder {
        client.patch(url)
    }

    pub fn put(client: &Client, url: impl IntoUrl) -> RequestBuilder {
        client.put(url)
    }
}
