use std::collections::HashMap;
use std::future::IntoFuture;
use std::pin::Pin;
use std::sync::Mutex;

use twilight_model::channel::message::MessageType;
use twilight_model::channel::{
    Channel,
    message::{Embed, Message, MessageFlags},
};
use twilight_model::http::interaction::InteractionResponse;
use twilight_model::id::{
    Id,
    marker::{
        ApplicationMarker, ChannelMarker, GuildMarker, InteractionMarker, MessageMarker,
        RoleMarker, UserMarker,
    },
};
use twilight_model::oauth::Application;
use twilight_model::user::User;
use twilight_model::util::Timestamp;

use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub content: Option<String>,
    pub embeds: Vec<Embed>,
    pub kind: MessageOp,
}

#[derive(Debug, Clone, Copy)]
pub enum MessageOp {
    Create,
    Update,
}

#[derive(Debug, Clone)]
pub struct InteractionRecord {
    pub application_id: Id<ApplicationMarker>,
    pub interaction_id: Id<InteractionMarker>,
    pub token: String,
    pub response: InteractionResponse,
}

pub struct MockResponse<T> {
    data: T,
}

impl<T> MockResponse<T> {
    pub fn new(data: T) -> Self {
        Self { data }
    }
    pub async fn model(self) -> anyhow::Result<T> {
        Ok(self.data)
    }
}

pub struct MockCreateMessage<'a> {
    client: &'a MockClient,
    channel_id: Id<ChannelMarker>,
    content: Option<String>,
    embeds: Vec<Embed>,
}

impl<'a> MockCreateMessage<'a> {
    pub fn content(mut self, content: &str) -> Self {
        self.content = Some(content.to_string());
        self
    }
    pub fn embeds(mut self, embeds: &'a [Embed]) -> Self {
        self.embeds = embeds.to_vec();
        self
    }
    pub fn flags(self, _flags: MessageFlags) -> Self {
        self
    }
    async fn exec(self) -> anyhow::Result<MockResponse<Message>> {
        let id = self.client.next_id.fetch_add(1, Ordering::SeqCst);
        let message = fake_message(
            Id::new(id),
            self.channel_id,
            self.content.clone().unwrap_or_default(),
            self.embeds.clone(),
        );
        let record = MessageRecord {
            channel_id: self.channel_id,
            message_id: Id::new(id),
            content: self.content,
            embeds: self.embeds,
            kind: MessageOp::Create,
        };
        self.client.messages.lock().unwrap().push(record);
        Ok(MockResponse::new(message))
    }
}

impl<'a> IntoFuture for MockCreateMessage<'a> {
    type Output = anyhow::Result<MockResponse<Message>>;
    type IntoFuture = Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.exec())
    }
}

pub struct MockUpdateMessage<'a> {
    client: &'a MockClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    content: Option<Option<String>>,
    embeds: Option<Vec<Embed>>,
}

impl<'a> MockUpdateMessage<'a> {
    pub fn content(mut self, content: Option<&'a str>) -> Self {
        self.content = Some(content.map(|s| s.to_string()));
        self
    }
    pub fn embeds(mut self, embeds: Option<&'a [Embed]>) -> Self {
        self.embeds = embeds.map(|e| e.to_vec());
        self
    }
    pub fn components(
        self,
        _components: Option<&'a [twilight_model::channel::message::component::Component]>,
    ) -> Self {
        self
    }

    async fn exec(self) -> anyhow::Result<MockResponse<Message>> {
        let content = self.content.unwrap_or(None);
        let embeds = self.embeds.unwrap_or_default();
        let message = fake_message(
            self.message_id,
            self.channel_id,
            content.clone().unwrap_or_default(),
            embeds.clone(),
        );
        let record = MessageRecord {
            channel_id: self.channel_id,
            message_id: self.message_id,
            content,
            embeds,
            kind: MessageOp::Update,
        };
        self.client.messages.lock().unwrap().push(record);
        Ok(MockResponse::new(message))
    }
}

impl<'a> IntoFuture for MockUpdateMessage<'a> {
    type Output = anyhow::Result<MockResponse<Message>>;
    type IntoFuture = Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.exec())
    }
}

pub struct MockInteractionClient<'a> {
    client: &'a MockClient,
    application_id: Id<ApplicationMarker>,
}

impl<'a> MockInteractionClient<'a> {
    pub fn create_response(
        &'a self,
        interaction_id: Id<InteractionMarker>,
        token: &'a str,
        resp: &'a InteractionResponse,
    ) -> MockInteractionResponseFuture<'a> {
        MockInteractionResponseFuture {
            client: self.client,
            record: InteractionRecord {
                application_id: self.application_id,
                interaction_id,
                token: token.to_string(),
                response: resp.clone(),
            },
        }
    }

    pub fn update_response(&'a self, _token: &'a str) -> MockUpdateMessage<'a> {
        MockUpdateMessage {
            client: self.client,
            channel_id: Id::new(0),
            message_id: Id::new(0),
            content: None,
            embeds: None,
        }
    }

    pub fn create_followup(&'a self, _token: &'a str) -> MockCreateMessage<'a> {
        MockCreateMessage {
            client: self.client,
            channel_id: Id::new(0),
            content: None,
            embeds: Vec::new(),
        }
    }

    pub async fn set_global_commands(
        &'a self,
        _commands: &[twilight_model::application::command::Command],
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct MockInteractionResponseFuture<'a> {
    client: &'a MockClient,
    record: InteractionRecord,
}

impl<'a> IntoFuture for MockInteractionResponseFuture<'a> {
    type Output = anyhow::Result<()>;
    type IntoFuture = Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            self.client.interactions.lock().unwrap().push(self.record);
            Ok(())
        })
    }
}

pub struct MockClient {
    pub messages: Mutex<Vec<MessageRecord>>,
    pub interactions: Mutex<Vec<InteractionRecord>>,
    pub channels: Mutex<HashMap<Id<ChannelMarker>, Vec<Message>>>,
    next_id: AtomicU64,
}

impl Default for MockClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockClient {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
            interactions: Mutex::new(Vec::new()),
            channels: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn add_channel_messages(&self, channel: Id<ChannelMarker>, msgs: Vec<Message>) {
        self.channels.lock().unwrap().insert(channel, msgs);
    }

    pub fn create_message(&self, channel_id: Id<ChannelMarker>) -> MockCreateMessage<'_> {
        MockCreateMessage {
            client: self,
            channel_id,
            content: None,
            embeds: Vec::new(),
        }
    }

    pub fn update_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> MockUpdateMessage<'_> {
        MockUpdateMessage {
            client: self,
            channel_id,
            message_id,
            content: None,
            embeds: None,
        }
    }

    pub async fn channel_messages(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> anyhow::Result<MockResponse<Vec<Message>>> {
        let map = self.channels.lock().unwrap();
        let data = map.get(&channel_id).cloned().unwrap_or_default();
        Ok(MockResponse::new(data))
    }

    pub async fn delete_message(
        &self,
        _channel_id: Id<ChannelMarker>,
        _message_id: Id<MessageMarker>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn delete_messages(
        &self,
        _channel_id: Id<ChannelMarker>,
        _messages: &[Id<MessageMarker>],
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn delete_all_reactions(
        &self,
        _channel_id: Id<ChannelMarker>,
        _message_id: Id<MessageMarker>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn add_guild_member_role(
        &self,
        _guild_id: Id<GuildMarker>,
        _user_id: Id<UserMarker>,
        _role_id: Id<RoleMarker>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn remove_guild_member_role(
        &self,
        _guild_id: Id<GuildMarker>,
        _user_id: Id<UserMarker>,
        _role_id: Id<RoleMarker>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> anyhow::Result<MockResponse<Message>> {
        let map = self.channels.lock().unwrap();
        let message = map
            .get(&channel_id)
            .and_then(|msgs| msgs.iter().find(|m| m.id == message_id).cloned())
            .unwrap_or_else(|| fake_message(message_id, channel_id, String::new(), Vec::new()));
        Ok(MockResponse::new(message))
    }

    pub async fn create_reaction(
        &self,
        _channel_id: Id<ChannelMarker>,
        _message_id: Id<MessageMarker>,
        _reaction: &twilight_http::request::channel::reaction::RequestReactionType<'_>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn create_private_channel(
        &self,
        _user_id: Id<UserMarker>,
    ) -> anyhow::Result<MockResponse<Channel>> {
        let channel = fake_channel(Id::new(0));
        Ok(MockResponse::new(channel))
    }

    pub async fn create_typing_trigger(
        &self,
        _channel_id: Id<ChannelMarker>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn interaction(&self, application_id: Id<ApplicationMarker>) -> MockInteractionClient<'_> {
        MockInteractionClient {
            client: self,
            application_id,
        }
    }

    pub async fn current_user_application(&self) -> anyhow::Result<MockResponse<Application>> {
        let app = fake_application(Id::new(1));
        Ok(MockResponse::new(app))
    }
}

fn fake_application(id: Id<ApplicationMarker>) -> Application {
    serde_json::from_value(json!({
        "bot_public": true,
        "bot_require_code_grant": false,
        "description": "",
        "icon": null,
        "id": id.to_string(),
        "name": "mock"
    }))
    .unwrap()
}

fn fake_channel(id: Id<ChannelMarker>) -> Channel {
    serde_json::from_value(json!({
        "id": id.to_string(),
        "type": 1
    }))
    .unwrap()
}

fn fake_user(id: Id<UserMarker>) -> User {
    serde_json::from_value(json!({
        "id": id.to_string(),
        "username": "mock",
        "discriminator": 1,
        "bot": false
    }))
    .unwrap()
}

fn fake_message(
    id: Id<MessageMarker>,
    channel_id: Id<ChannelMarker>,
    content: String,
    embeds: Vec<Embed>,
) -> Message {
    serde_json::from_value(json!({
        "id": id.to_string(),
        "channel_id": channel_id.to_string(),
        "attachments": [],
        "author": fake_user(Id::new(1)),
        "content": content,
        "components": [],
        "edited_timestamp": null,
        "embeds": embeds,
        "flags": 0,
        "kind": u8::from(MessageType::Regular),
        "mention_channels": [],
        "mention_everyone": false,
        "mention_roles": [],
        "mentions": [],
        "message_snapshots": [],
        "pinned": false,
        "reactions": [],
        "sticker_items": [],
        "timestamp": Timestamp::from_secs(0).unwrap().iso_8601().to_string(),
        "tts": false
    }))
    .unwrap()
}
