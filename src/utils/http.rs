use async_trait::async_trait;
use serde::de::DeserializeOwned;

#[async_trait]
pub trait HttpProvider {
    async fn get_json<T>(&self, url: &str) -> anyhow::Result<T>
    where
        T: DeserializeOwned + Send;
}

#[async_trait]
impl HttpProvider for reqwest::Client {
    async fn get_json<T>(&self, url: &str) -> anyhow::Result<T>
    where
        T: DeserializeOwned + Send,
    {
        let res = self.get(url).send().await?.error_for_status()?;
        Ok(res.json().await?)
    }
}
