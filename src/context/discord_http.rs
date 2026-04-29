use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::anyhow;
use tokio::sync::{Semaphore, mpsc, oneshot};
use twilight_http::Client as RawClient;
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_http::response::marker::ListBody;
use twilight_http::response::Response;
use twilight_model::application::command::Command;
use twilight_model::channel::{Channel, Message};
use twilight_model::channel::message::component::Component;
use twilight_model::channel::message::{Embed, MessageFlags};
use twilight_model::http::interaction::InteractionResponse;
use twilight_model::id::Id;
use twilight_model::id::marker::{
    ApplicationMarker, ChannelMarker, GuildMarker, InteractionMarker, MessageMarker, RoleMarker,
    UserMarker,
};
use twilight_model::oauth::Application;

const CRITICAL_QUEUE_CAP: usize = 128;
const HIGH_QUEUE_CAP: usize = 256;
const NORMAL_QUEUE_CAP: usize = 256;
const LOW_QUEUE_CAP: usize = 128;
const MAX_IN_FLIGHT: usize = 16;

type JobFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscordPriority {
    Critical,
    High,
    Normal,
    Low,
}

impl DiscordPriority {
    fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Normal => "normal",
            Self::Low => "low",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscordOpKind {
    CurrentUserApplication,
    SetGlobalCommands,
    CreatePrivateChannel,
    CreateTypingTrigger,
    CreateMessage,
    UpdateMessage,
    GetMessage,
    ListChannelMessages,
    DeleteMessage,
    DeleteMessages,
    CreateReaction,
    AddGuildMemberRole,
    RemoveGuildMemberRole,
    InteractionCreateResponse,
    InteractionUpdateResponse,
    InteractionCreateFollowup,
}

impl DiscordOpKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::CurrentUserApplication => "current_user_application",
            Self::SetGlobalCommands => "set_global_commands",
            Self::CreatePrivateChannel => "create_private_channel",
            Self::CreateTypingTrigger => "create_typing_trigger",
            Self::CreateMessage => "create_message",
            Self::UpdateMessage => "update_message",
            Self::GetMessage => "get_message",
            Self::ListChannelMessages => "list_channel_messages",
            Self::DeleteMessage => "delete_message",
            Self::DeleteMessages => "delete_messages",
            Self::CreateReaction => "create_reaction",
            Self::AddGuildMemberRole => "add_guild_member_role",
            Self::RemoveGuildMemberRole => "remove_guild_member_role",
            Self::InteractionCreateResponse => "interaction_create_response",
            Self::InteractionUpdateResponse => "interaction_update_response",
            Self::InteractionCreateFollowup => "interaction_create_followup",
        }
    }

    fn default_priority(self) -> DiscordPriority {
        match self {
            Self::InteractionCreateResponse
            | Self::InteractionUpdateResponse
            | Self::AddGuildMemberRole
            | Self::RemoveGuildMemberRole => DiscordPriority::Critical,
            Self::CurrentUserApplication
            | Self::SetGlobalCommands
            | Self::CreatePrivateChannel
            | Self::CreateTypingTrigger
            | Self::InteractionCreateFollowup => DiscordPriority::High,
            Self::CreateMessage | Self::UpdateMessage | Self::GetMessage | Self::CreateReaction => {
                DiscordPriority::Normal
            }
            Self::ListChannelMessages | Self::DeleteMessage | Self::DeleteMessages => {
                DiscordPriority::Low
            }
        }
    }
}

struct Job {
    priority: DiscordPriority,
    enqueued_at: Instant,
    run: JobFuture,
}

enum OwnedReaction {
    Custom {
        id: Id<twilight_model::id::marker::EmojiMarker>,
        name: Option<String>,
    },
    Unicode {
        name: String,
    },
}

#[derive(Default)]
struct QueueDepth {
    critical: AtomicUsize,
    high: AtomicUsize,
    normal: AtomicUsize,
    low: AtomicUsize,
    in_flight: AtomicUsize,
}

impl QueueDepth {
    fn increment(&self, priority: DiscordPriority) {
        let depth = match priority {
            DiscordPriority::Critical => self.critical.fetch_add(1, Ordering::Relaxed) + 1,
            DiscordPriority::High => self.high.fetch_add(1, Ordering::Relaxed) + 1,
            DiscordPriority::Normal => self.normal.fetch_add(1, Ordering::Relaxed) + 1,
            DiscordPriority::Low => self.low.fetch_add(1, Ordering::Relaxed) + 1,
        };
        metrics::gauge!("discord_http_queue_depth", "priority" => priority.as_str()).set(depth as f64);
    }

    fn decrement(&self, priority: DiscordPriority) {
        let depth = match priority {
            DiscordPriority::Critical => {
                self.critical.fetch_sub(1, Ordering::Relaxed).saturating_sub(1)
            }
            DiscordPriority::High => self.high.fetch_sub(1, Ordering::Relaxed).saturating_sub(1),
            DiscordPriority::Normal => {
                self.normal.fetch_sub(1, Ordering::Relaxed).saturating_sub(1)
            }
            DiscordPriority::Low => self.low.fetch_sub(1, Ordering::Relaxed).saturating_sub(1),
        };
        metrics::gauge!("discord_http_queue_depth", "priority" => priority.as_str()).set(depth as f64);
    }

    fn in_flight_inc(&self) {
        let count = self.in_flight.fetch_add(1, Ordering::Relaxed) + 1;
        metrics::gauge!("discord_http_in_flight").set(count as f64);
    }

    fn in_flight_dec(&self) {
        let count = self.in_flight.fetch_sub(1, Ordering::Relaxed).saturating_sub(1);
        metrics::gauge!("discord_http_in_flight").set(count as f64);
    }
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    raw: Arc<RawClient>,
    critical_tx: mpsc::Sender<Job>,
    high_tx: mpsc::Sender<Job>,
    normal_tx: mpsc::Sender<Job>,
    low_tx: mpsc::Sender<Job>,
    depth: Arc<QueueDepth>,
}

impl Client {
    pub fn new(raw: RawClient) -> Self {
        let (critical_tx, critical_rx) = mpsc::channel(CRITICAL_QUEUE_CAP);
        let (high_tx, high_rx) = mpsc::channel(HIGH_QUEUE_CAP);
        let (normal_tx, normal_rx) = mpsc::channel(NORMAL_QUEUE_CAP);
        let (low_tx, low_rx) = mpsc::channel(LOW_QUEUE_CAP);

        let depth = Arc::new(QueueDepth::default());
        let raw = Arc::new(raw);

        tokio::spawn(Self::dispatch_loop(
            critical_rx,
            high_rx,
            normal_rx,
            low_rx,
            depth.clone(),
        ));

        Self {
            inner: Arc::new(ClientInner {
                raw,
                critical_tx,
                high_tx,
                normal_tx,
                low_tx,
                depth,
            }),
        }
    }

    async fn dispatch_loop(
        mut critical_rx: mpsc::Receiver<Job>,
        mut high_rx: mpsc::Receiver<Job>,
        mut normal_rx: mpsc::Receiver<Job>,
        mut low_rx: mpsc::Receiver<Job>,
        depth: Arc<QueueDepth>,
    ) {
        const SCHEDULE: [DiscordPriority; 15] = [
            DiscordPriority::Critical,
            DiscordPriority::Critical,
            DiscordPriority::Critical,
            DiscordPriority::Critical,
            DiscordPriority::Critical,
            DiscordPriority::Critical,
            DiscordPriority::Critical,
            DiscordPriority::Critical,
            DiscordPriority::High,
            DiscordPriority::High,
            DiscordPriority::High,
            DiscordPriority::High,
            DiscordPriority::Normal,
            DiscordPriority::Normal,
            DiscordPriority::Low,
        ];

        let sem = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
        let mut cursor = 0usize;

        loop {
            let job = match Self::next_job(
                &mut critical_rx,
                &mut high_rx,
                &mut normal_rx,
                &mut low_rx,
                &mut cursor,
                &SCHEDULE,
            )
            .await
            {
                Some(job) => job,
                None => break,
            };

            let permit = match sem.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => break,
            };

            depth.decrement(job.priority);
            depth.in_flight_inc();
            metrics::histogram!(
                "discord_http_queue_wait_seconds",
                "priority" => job.priority.as_str()
            )
            .record(job.enqueued_at.elapsed().as_secs_f64());

            let depth2 = depth.clone();
            tokio::spawn(async move {
                let _permit = permit;
                job.run.await;
                depth2.in_flight_dec();
            });
        }
    }

    async fn next_job(
        critical_rx: &mut mpsc::Receiver<Job>,
        high_rx: &mut mpsc::Receiver<Job>,
        normal_rx: &mut mpsc::Receiver<Job>,
        low_rx: &mut mpsc::Receiver<Job>,
        cursor: &mut usize,
        schedule: &[DiscordPriority],
    ) -> Option<Job> {
        for _ in 0..schedule.len() {
            let priority = schedule[*cursor];
            *cursor = (*cursor + 1) % schedule.len();
            let item = match priority {
                DiscordPriority::Critical => critical_rx.try_recv().ok(),
                DiscordPriority::High => high_rx.try_recv().ok(),
                DiscordPriority::Normal => normal_rx.try_recv().ok(),
                DiscordPriority::Low => low_rx.try_recv().ok(),
            };
            if item.is_some() {
                return item;
            }
        }

        tokio::select! {
            biased;
            item = critical_rx.recv() => item,
            item = high_rx.recv() => item,
            item = normal_rx.recv() => item,
            item = low_rx.recv() => item,
        }
    }

    async fn execute<Op, Fut, T, E>(
        &self,
        priority: DiscordPriority,
        op_kind: DiscordOpKind,
        op: Op,
    ) -> anyhow::Result<T>
    where
        Op: FnOnce(Arc<RawClient>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        T: Send + 'static,
        E: Into<anyhow::Error> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let client = self.inner.raw.clone();
        let enqueued_at = Instant::now();
        let run = Box::pin(async move {
            let started_at = Instant::now();
            let result = op(client).await.map_err(Into::into);
            metrics::histogram!(
                "discord_http_execution_seconds",
                "priority" => priority.as_str(),
                "op" => op_kind.as_str()
            )
            .record(started_at.elapsed().as_secs_f64());
            metrics::counter!(
                "discord_http_requests_total",
                "priority" => priority.as_str(),
                "op" => op_kind.as_str(),
                "result" => if result.is_ok() { "ok" } else { "error" }
            )
            .increment(1);
            let _ = tx.send(result);
        });

        let job = Job { priority, enqueued_at, run };
        self.inner.depth.increment(priority);
        self.sender(priority)
            .send(job)
            .await
            .map_err(|_| anyhow!("discord http scheduler queue closed"))?;

        rx.await
            .map_err(|_| anyhow!("discord http scheduler worker dropped response"))?
    }

    fn sender(&self, priority: DiscordPriority) -> &mpsc::Sender<Job> {
        match priority {
            DiscordPriority::Critical => &self.inner.critical_tx,
            DiscordPriority::High => &self.inner.high_tx,
            DiscordPriority::Normal => &self.inner.normal_tx,
            DiscordPriority::Low => &self.inner.low_tx,
        }
    }

    pub async fn current_user_application(&self) -> anyhow::Result<Response<Application>> {
        self.execute(
            DiscordOpKind::CurrentUserApplication.default_priority(),
            DiscordOpKind::CurrentUserApplication,
            |client| async move { client.current_user_application().await },
        )
        .await
    }

    pub fn interaction(&self, application_id: Id<ApplicationMarker>) -> InteractionClient {
        InteractionClient { http: self.clone(), application_id }
    }

    pub fn create_message(&self, channel_id: Id<ChannelMarker>) -> CreateMessage {
        CreateMessage::new(self.clone(), channel_id, DiscordOpKind::CreateMessage.default_priority())
    }

    pub fn update_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> UpdateMessage {
        UpdateMessage::new(
            self.clone(),
            channel_id,
            message_id,
            DiscordOpKind::UpdateMessage.default_priority(),
        )
    }

    pub async fn channel_messages(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> anyhow::Result<Response<ListBody<Message>>> {
        self.execute(
            DiscordOpKind::ListChannelMessages.default_priority(),
            DiscordOpKind::ListChannelMessages,
            move |client| async move { client.channel_messages(channel_id).await },
        )
        .await
    }

    pub async fn delete_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> anyhow::Result<()> {
        self.execute(
            DiscordOpKind::DeleteMessage.default_priority(),
            DiscordOpKind::DeleteMessage,
            move |client| async move {
                client
                    .delete_message(channel_id, message_id)
                    .await
                    .map(|_| ())
            },
        )
        .await
    }

    pub async fn delete_messages(
        &self,
        channel_id: Id<ChannelMarker>,
        messages: &[Id<MessageMarker>],
    ) -> anyhow::Result<()> {
        let messages = messages.to_vec();
        self.execute(
            DiscordOpKind::DeleteMessages.default_priority(),
            DiscordOpKind::DeleteMessages,
            move |client| async move {
                client
                    .delete_messages(channel_id, &messages)
                    .await
                    .map(|_| ())
            },
        )
        .await
    }

    pub async fn message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> anyhow::Result<Response<Message>> {
        self.execute(
            DiscordOpKind::GetMessage.default_priority(),
            DiscordOpKind::GetMessage,
            move |client| async move { client.message(channel_id, message_id).await },
        )
        .await
    }

    pub async fn create_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        reaction: &RequestReactionType<'_>,
    ) -> anyhow::Result<()> {
        let reaction = match reaction {
            RequestReactionType::Custom { id, name } => OwnedReaction::Custom {
                id: *id,
                name: name.map(ToString::to_string),
            },
            RequestReactionType::Unicode { name } => OwnedReaction::Unicode {
                name: (*name).to_string(),
            },
        };
        self.execute(
            DiscordOpKind::CreateReaction.default_priority(),
            DiscordOpKind::CreateReaction,
            move |client| async move {
                let request = match &reaction {
                    OwnedReaction::Custom { id, name } => RequestReactionType::Custom {
                        id: *id,
                        name: name.as_deref(),
                    },
                    OwnedReaction::Unicode { name } => RequestReactionType::Unicode { name },
                };
                client
                    .create_reaction(channel_id, message_id, &request)
                    .await
                    .map(|_| ())
            },
        )
        .await
    }

    pub async fn create_private_channel(
        &self,
        user_id: Id<UserMarker>,
    ) -> anyhow::Result<Response<Channel>> {
        self.execute(
            DiscordOpKind::CreatePrivateChannel.default_priority(),
            DiscordOpKind::CreatePrivateChannel,
            move |client| async move { client.create_private_channel(user_id).await },
        )
        .await
    }

    pub async fn create_typing_trigger(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> anyhow::Result<()> {
        self.execute(
            DiscordOpKind::CreateTypingTrigger.default_priority(),
            DiscordOpKind::CreateTypingTrigger,
            move |client| async move { client.create_typing_trigger(channel_id).await.map(|_| ()) },
        )
        .await
    }

    pub async fn add_guild_member_role(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        role_id: Id<RoleMarker>,
    ) -> anyhow::Result<()> {
        self.execute(
            DiscordOpKind::AddGuildMemberRole.default_priority(),
            DiscordOpKind::AddGuildMemberRole,
            move |client| async move {
                client
                    .add_guild_member_role(guild_id, user_id, role_id)
                    .await
                    .map(|_| ())
            },
        )
        .await
    }

    pub async fn remove_guild_member_role(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        role_id: Id<RoleMarker>,
    ) -> anyhow::Result<()> {
        self.execute(
            DiscordOpKind::RemoveGuildMemberRole.default_priority(),
            DiscordOpKind::RemoveGuildMemberRole,
            move |client| async move {
                client
                    .remove_guild_member_role(guild_id, user_id, role_id)
                    .await
                    .map(|_| ())
            },
        )
        .await
    }
}

#[derive(Clone)]
pub struct InteractionClient {
    http: Client,
    application_id: Id<ApplicationMarker>,
}

impl InteractionClient {
    pub async fn create_response(
        &self,
        interaction_id: Id<InteractionMarker>,
        token: &str,
        response: &InteractionResponse,
    ) -> anyhow::Result<()> {
        let application_id = self.application_id;
        let token = token.to_string();
        let response = response.clone();
        self.http
            .execute(
                DiscordOpKind::InteractionCreateResponse.default_priority(),
                DiscordOpKind::InteractionCreateResponse,
                move |client| async move {
                    client
                        .interaction(application_id)
                        .create_response(interaction_id, &token, &response)
                        .await
                        .map(|_| ())
                },
            )
            .await
    }

    pub fn update_response(&self, token: &str) -> UpdateResponse {
        UpdateResponse::new(
            self.http.clone(),
            self.application_id,
            token.to_string(),
            DiscordOpKind::InteractionUpdateResponse.default_priority(),
        )
    }

    pub fn create_followup(&self, token: &str) -> CreateFollowup {
        CreateFollowup::new(
            self.http.clone(),
            self.application_id,
            token.to_string(),
            DiscordOpKind::InteractionCreateFollowup.default_priority(),
        )
    }

    pub async fn set_global_commands(&self, commands: &[Command]) -> anyhow::Result<()> {
        let application_id = self.application_id;
        let commands = commands.to_vec();
        self.http
            .execute(
                DiscordOpKind::SetGlobalCommands.default_priority(),
                DiscordOpKind::SetGlobalCommands,
                move |client| async move {
                    client
                        .interaction(application_id)
                        .set_global_commands(&commands)
                        .await
                        .map(|_| ())
                },
            )
            .await
    }
}

pub struct CreateMessage {
    http: Client,
    channel_id: Id<ChannelMarker>,
    content: Option<String>,
    embeds: Vec<Embed>,
    components: Vec<Component>,
    flags: Option<MessageFlags>,
    priority: DiscordPriority,
}

impl CreateMessage {
    fn new(http: Client, channel_id: Id<ChannelMarker>, priority: DiscordPriority) -> Self {
        Self {
            http,
            channel_id,
            content: None,
            embeds: Vec::new(),
            components: Vec::new(),
            flags: None,
            priority,
        }
    }

    pub fn content(mut self, content: &str) -> Self {
        self.content = Some(content.to_string());
        self
    }

    pub fn embeds(mut self, embeds: &[Embed]) -> Self {
        self.embeds = embeds.to_vec();
        self
    }

    pub fn components(mut self, components: &[Component]) -> Self {
        self.components = components.to_vec();
        self
    }

    pub fn flags(mut self, flags: MessageFlags) -> Self {
        self.flags = Some(flags);
        self
    }

    pub fn priority(mut self, priority: DiscordPriority) -> Self {
        self.priority = priority;
        self
    }
}

impl IntoFuture for CreateMessage {
    type Output = anyhow::Result<Response<Message>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'static>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            self.http
                .execute(self.priority, DiscordOpKind::CreateMessage, move |client| async move {
                    let mut req = client.create_message(self.channel_id);
                    if let Some(content) = &self.content {
                        req = req.content(content);
                    }
                    if !self.embeds.is_empty() {
                        req = req.embeds(&self.embeds);
                    }
                    if !self.components.is_empty() {
                        req = req.components(&self.components);
                    }
                    if let Some(flags) = self.flags {
                        req = req.flags(flags);
                    }
                    req.await
                })
                .await
        })
    }
}

pub struct UpdateMessage {
    http: Client,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    content: Option<Option<String>>,
    embeds: Option<Option<Vec<Embed>>>,
    components: Option<Option<Vec<Component>>>,
    priority: DiscordPriority,
}

impl UpdateMessage {
    fn new(
        http: Client,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        priority: DiscordPriority,
    ) -> Self {
        Self {
            http,
            channel_id,
            message_id,
            content: None,
            embeds: None,
            components: None,
            priority,
        }
    }

    pub fn content(mut self, content: Option<&str>) -> Self {
        self.content = Some(content.map(ToString::to_string));
        self
    }

    pub fn embeds(mut self, embeds: Option<&[Embed]>) -> Self {
        self.embeds = Some(embeds.map(|items| items.to_vec()));
        self
    }

    pub fn components(mut self, components: Option<&[Component]>) -> Self {
        self.components = Some(components.map(|items| items.to_vec()));
        self
    }

    pub fn priority(mut self, priority: DiscordPriority) -> Self {
        self.priority = priority;
        self
    }
}

impl IntoFuture for UpdateMessage {
    type Output = anyhow::Result<Response<Message>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'static>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            self.http
                .execute(self.priority, DiscordOpKind::UpdateMessage, move |client| async move {
                    let mut req = client.update_message(self.channel_id, self.message_id);
                    if let Some(content) = &self.content {
                        req = req.content(content.as_deref());
                    }
                    if let Some(embeds) = &self.embeds {
                        req = req.embeds(embeds.as_deref());
                    }
                    if let Some(components) = &self.components {
                        req = req.components(components.as_deref());
                    }
                    req.await
                })
                .await
        })
    }
}

pub struct UpdateResponse {
    http: Client,
    application_id: Id<ApplicationMarker>,
    token: String,
    content: Option<Option<String>>,
    embeds: Option<Option<Vec<Embed>>>,
    components: Option<Option<Vec<Component>>>,
    priority: DiscordPriority,
}

impl UpdateResponse {
    fn new(
        http: Client,
        application_id: Id<ApplicationMarker>,
        token: String,
        priority: DiscordPriority,
    ) -> Self {
        Self {
            http,
            application_id,
            token,
            content: None,
            embeds: None,
            components: None,
            priority,
        }
    }

    pub fn content(mut self, content: Option<&str>) -> Self {
        self.content = Some(content.map(ToString::to_string));
        self
    }

    pub fn embeds(mut self, embeds: Option<&[Embed]>) -> Self {
        self.embeds = Some(embeds.map(|items| items.to_vec()));
        self
    }

    pub fn components(mut self, components: Option<&[Component]>) -> Self {
        self.components = Some(components.map(|items| items.to_vec()));
        self
    }

    pub fn priority(mut self, priority: DiscordPriority) -> Self {
        self.priority = priority;
        self
    }
}

impl IntoFuture for UpdateResponse {
    type Output = anyhow::Result<Response<Message>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'static>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            self.http
                .execute(
                    self.priority,
                    DiscordOpKind::InteractionUpdateResponse,
                    move |client| async move {
                        let interaction = client.interaction(self.application_id);
                        let mut req = interaction.update_response(&self.token);
                        if let Some(content) = &self.content {
                            req = req.content(content.as_deref());
                        }
                        if let Some(embeds) = &self.embeds {
                            req = req.embeds(embeds.as_deref());
                        }
                        if let Some(components) = &self.components {
                            req = req.components(components.as_deref());
                        }
                        req.await
                    },
                )
                .await
        })
    }
}

pub struct CreateFollowup {
    http: Client,
    application_id: Id<ApplicationMarker>,
    token: String,
    content: Option<String>,
    embeds: Vec<Embed>,
    components: Vec<Component>,
    flags: Option<MessageFlags>,
    priority: DiscordPriority,
}

impl CreateFollowup {
    fn new(
        http: Client,
        application_id: Id<ApplicationMarker>,
        token: String,
        priority: DiscordPriority,
    ) -> Self {
        Self {
            http,
            application_id,
            token,
            content: None,
            embeds: Vec::new(),
            components: Vec::new(),
            flags: None,
            priority,
        }
    }

    pub fn content(mut self, content: &str) -> Self {
        self.content = Some(content.to_string());
        self
    }

    pub fn embeds(mut self, embeds: &[Embed]) -> Self {
        self.embeds = embeds.to_vec();
        self
    }

    pub fn components(mut self, components: &[Component]) -> Self {
        self.components = components.to_vec();
        self
    }

    pub fn flags(mut self, flags: MessageFlags) -> Self {
        self.flags = Some(flags);
        self
    }

    pub fn priority(mut self, priority: DiscordPriority) -> Self {
        self.priority = priority;
        self
    }
}

impl IntoFuture for CreateFollowup {
    type Output = anyhow::Result<Response<Message>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'static>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            self.http
                .execute(
                    self.priority,
                    DiscordOpKind::InteractionCreateFollowup,
                    move |client| async move {
                        let interaction = client.interaction(self.application_id);
                        let mut req = interaction.create_followup(&self.token);
                        if let Some(content) = &self.content {
                            req = req.content(content);
                        }
                        if !self.embeds.is_empty() {
                            req = req.embeds(&self.embeds);
                        }
                        if !self.components.is_empty() {
                            req = req.components(&self.components);
                        }
                        if let Some(flags) = self.flags {
                            req = req.flags(flags);
                        }
                        req.await
                    },
                )
                .await
        })
    }
}
