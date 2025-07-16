#![allow(dead_code)]

use anyhow::Result;
use std::sync::{Arc, Mutex};
use twilight_model::{
    http::interaction::InteractionResponse,
    id::{
        Id,
        marker::{ApplicationMarker, ChannelMarker, InteractionMarker, UserMarker},
    },
};

#[derive(Clone, Default)]
pub struct MockHttp {
    pub logs: Arc<Mutex<Vec<String>>>,
    pub dm_ok: bool,
}

impl MockHttp {
    pub fn new() -> Self {
        Self {
            logs: Arc::new(Mutex::new(Vec::new())),
            dm_ok: true,
        }
    }

    pub fn clear(&self) {
        self.logs.lock().unwrap().clear();
    }

    pub fn interaction(&self, _app_id: Id<ApplicationMarker>) -> InteractionHandle {
        InteractionHandle { http: self.clone() }
    }

    pub async fn create_private_channel(&self, _user: Id<UserMarker>) -> Result<CreateChannelResp> {
        self.logs
            .lock()
            .unwrap()
            .push("create_private_channel".into());
        if self.dm_ok {
            Ok(CreateChannelResp {
                _http: self.clone(),
                channel: Channel { id: Id::new(1) },
            })
        } else {
            Err(anyhow::anyhow!("dm failed"))
        }
    }

    pub fn create_message(&self, _channel_id: Id<ChannelMarker>) -> MessageBuilder {
        self.logs.lock().unwrap().push("create_message".into());
        MessageBuilder
    }
}

#[derive(Clone)]
pub struct InteractionHandle {
    http: MockHttp,
}

impl InteractionHandle {
    pub async fn create_response(
        &self,
        _id: Id<InteractionMarker>,
        _token: &str,
        _resp: &InteractionResponse,
    ) -> Result<()> {
        self.http
            .logs
            .lock()
            .unwrap()
            .push("create_response".into());
        Ok(())
    }
}

pub struct CreateChannelResp {
    _http: MockHttp,
    channel: Channel,
}

impl CreateChannelResp {
    pub async fn model(self) -> Result<Channel> {
        Ok(self.channel)
    }
}

pub struct Channel {
    pub id: Id<ChannelMarker>,
}

pub struct MessageBuilder;
impl MessageBuilder {
    pub async fn content(self, _content: &str) -> Result<()> {
        Ok(())
    }
}
