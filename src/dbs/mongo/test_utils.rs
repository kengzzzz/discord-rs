use anyhow::Result;
use deadpool_redis::Pool;
use futures::Stream;
use mongodb::bson::{Document, to_document};
use serde::{Serialize, de::DeserializeOwned};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::RwLock;

use super::models::{
    ai_prompt::AiPrompt, channel::Channel, message::Message, quarantine::Quarantine, role::Role,
};

#[derive(Clone)]
pub struct MockCollection<T> {
    data: Arc<RwLock<Vec<T>>>,
}

impl<T> Default for MockCollection<T> {
    fn default() -> Self {
        Self { data: Arc::new(RwLock::new(Vec::new())) }
    }
}

impl<T> MockCollection<T>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self { data: Arc::new(RwLock::new(Vec::new())) }
    }

    pub async fn insert_one(&self, doc: T) -> Result<()> {
        self.data.write().await.push(doc);
        Ok(())
    }

    pub async fn find_one(&self, filter: Document) -> Result<Option<T>> {
        let data = self.data.read().await;
        for item in data.iter() {
            let doc = to_document(item)?;
            if matches(&doc, &filter) {
                return Ok(Some(item.clone()));
            }
        }
        Ok(None)
    }

    pub async fn find(&self, filter: Document) -> Result<MockCursor<T>> {
        let data = self.data.read().await;
        let mut vec = Vec::new();
        for item in data.iter() {
            let doc = to_document(item)?;
            if matches(&doc, &filter) {
                vec.push(item.clone());
            }
        }
        Ok(MockCursor { data: vec, index: 0 })
    }

    pub async fn delete_one(&self, filter: Document) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(pos) = data
            .iter()
            .position(|item| matches(&to_document(item).unwrap(), &filter))
        {
            data.remove(pos);
        }
        Ok(())
    }

    pub async fn delete_many(&self, filter: Document) -> Result<()> {
        let mut data = self.data.write().await;
        data.retain(|item| !matches(&to_document(item).unwrap(), &filter));
        Ok(())
    }

    async fn update_one_impl(
        &self,
        filter: Document,
        update: Document,
        upsert: bool,
    ) -> Result<()> {
        let mut data = self.data.write().await;
        for item in data.iter_mut() {
            let mut doc = to_document(item)?;
            if matches(&doc, &filter) {
                if let Ok(set) = update.get_document("$set") {
                    for (k, v) in set.iter() {
                        doc.insert(k, v.clone());
                    }
                    *item = mongodb::bson::from_document(doc)?;
                }
                return Ok(());
            }
        }
        if upsert && let Ok(set) = update.get_document("$set") {
            let mut doc = Document::new();
            for (k, v) in set.iter() {
                doc.insert(k, v.clone());
            }
            let item: T = mongodb::bson::from_document(doc)?;
            data.push(item);
        }
        Ok(())
    }

    pub fn update_one(&self, filter: Document, update: Document) -> UpdateOneBuilder<'_, T> {
        UpdateOneBuilder { collection: self, filter, update }
    }
}

pub struct UpdateOneBuilder<'a, T> {
    collection: &'a MockCollection<T>,
    filter: Document,
    update: Document,
}

impl<'a, T> UpdateOneBuilder<'a, T>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    pub fn upsert(self, upsert: bool) -> futures::future::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.collection
                .update_one_impl(self.filter, self.update, upsert)
                .await
        })
    }
}

pub struct MockCursor<T> {
    data: Vec<T>,
    index: usize,
}

impl<T> Stream for MockCursor<T>
where
    T: Clone + Unpin,
{
    type Item = Result<T>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.index >= self.data.len() {
            return Poll::Ready(None);
        }
        let item = self.data[self.index].clone();
        self.index += 1;
        Poll::Ready(Some(Ok(item)))
    }
}

fn matches(doc: &Document, filter: &Document) -> bool {
    filter
        .iter()
        .all(|(k, v)| doc.get(k) == Some(v))
}

#[derive(Clone, Default)]
pub struct MongoDB {
    pub channels: MockCollection<Channel>,
    pub roles: MockCollection<Role>,
    pub quarantines: MockCollection<Quarantine>,
    pub messages: MockCollection<Message>,
    pub ai_prompts: MockCollection<AiPrompt>,
}

impl MongoDB {
    pub async fn init(_redis: Pool, _watchers: bool) -> Result<Self> {
        Ok(Self::default())
    }

    pub fn client(&self) -> &mongodb::Client {
        unimplemented!("mock mongodb does not provide client")
    }
}
