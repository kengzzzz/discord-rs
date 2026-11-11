use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use anyhow::Context as _;
use tokio::sync::Semaphore;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use twilight_cache_inmemory::{DefaultInMemoryCache, ResourceType};
use twilight_model::{
    channel::message::AllowedMentions,
    id::{
        Id,
        marker::{ApplicationMarker, ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
};

use crate::{
    ai::Ai,
    config::Config,
    db::Db,
    features::relay::Relay,
    guild::{GuildConfig, Guilds},
    health::Health,
    moderation::Moderation,
    warframe::Warframe,
};

#[derive(Clone, Copy)]
pub enum Reaction {
    Busy,
    Relayed,
}

#[derive(Default)]
pub struct Budget(std::sync::Mutex<Option<(Instant, u32)>>);

impl Budget {
    fn take(&self, per_minute: u32) -> bool {
        let mut window = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        match &mut *window {
            Some((start, used)) if now.duration_since(*start) < Duration::from_secs(60) => {
                *used += 1;
                *used <= per_minute
            }
            _ => {
                *window = Some((now, 1));
                true
            }
        }
    }
}

pub const USER_AGENT: &str = concat!(
    "discord-rs/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/kengzzzz/discord-rs)"
);

pub struct App {
    pub config: Config,
    pub http: twilight_http::Client,
    /// No rate limiter: twilight's queues every callback one at a time, but Discord limits them
    /// per interaction.
    pub callbacks: twilight_http::Client,
    pub cache: DefaultInMemoryCache,
    pub application_id: Id<ApplicationMarker>,
    pub bot_id: Id<UserMarker>,
    pub db: Db,
    pub guilds: Guilds,
    pub web: reqwest::Client,
    pub moderation: Moderation,
    pub relay: Relay,
    pub ai: Ai,
    pub images: crate::image_worker::ImageWorker,
    pub warframe: Warframe,
    pub health: Health,
    pub tasks: TaskTracker,
    pub shutdown: CancellationToken,
    pub event_permits: Semaphore,
    pub event_backlog: Arc<Semaphore>,
    pub outbound: Arc<Semaphore>,
    pub relay_posts: Arc<Semaphore>,
    pub cleanup: Arc<Semaphore>,
    pub joins: Pending<(Id<GuildMarker>, Id<UserMarker>)>,
    /// Capped, unlike twilight's cache, which keeps every voice state.
    pub voices: std::sync::Mutex<crate::features::voice_log::Voices>,
    pub interactions: Interactions,
    /// Serializes card posts, so an old card's delete isn't mistaken for a user deleting the
    /// live one.
    pub bot_messages: tokio::sync::Mutex<()>,
    pub picker_retry: std::sync::Mutex<std::collections::HashSet<Id<GuildMarker>>>,
    /// Capped per minute: Discord counts refused reactions toward an IP ban.
    pub reactions: [Budget; 2],
    pub started_at: i64,
    pub(crate) started: OnceLock<()>,
}

impl App {
    pub async fn build(config: Config, shutdown: CancellationToken) -> anyhow::Result<Arc<Self>> {
        let db = Db::open(&config.database_path)?;
        let guilds = Guilds::load(db.clone()).await?;

        let http = twilight_http::Client::builder()
            .token(config.discord_token.clone())
            .default_allowed_mentions(AllowedMentions::default())
            .timeout(Duration::from_secs(15))
            .build();
        let callbacks = twilight_http::Client::builder()
            .token(config.discord_token.clone())
            .default_allowed_mentions(AllowedMentions::default())
            .timeout(Duration::from_secs(15))
            .ratelimiter(None)
            .build();
        let application = retry(&shutdown, "fetch application", || async {
            crate::discord::read(http.current_user_application().await?).await
        })
        .await
        .context("fetch application (is DISCORD_TOKEN valid?)")?;
        let bot = retry(&shutdown, "fetch bot user", || async {
            crate::discord::read(http.current_user().await?).await
        })
        .await?;

        let web = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()?;

        let cache = DefaultInMemoryCache::builder()
            .resource_types(
                ResourceType::GUILD
                    | ResourceType::CHANNEL
                    | ResourceType::ROLE
                    | ResourceType::MEMBER
                    | ResourceType::USER
                    | ResourceType::USER_CURRENT,
            )
            .build();

        Ok(Arc::new(Self {
            moderation: Moderation::new(&config.scam)?,
            ai: Ai::new(web.clone(), &config.ai),
            images: crate::image_worker::ImageWorker::new("images"),
            warframe: Warframe::default(),
            relay: Relay::default(),
            health: Health::default(),
            config,
            http,
            callbacks,
            cache,
            application_id: application.id,
            bot_id: bot.id,
            db,
            guilds,
            web,
            tasks: TaskTracker::new(),
            shutdown,
            event_permits: Semaphore::new(64),
            event_backlog: Arc::new(Semaphore::new(
                crate::gateway::EVENT_BACKLOG_KIB as usize,
            )),
            outbound: Arc::new(Semaphore::new(OUTBOUND_LIMIT)),
            relay_posts: Arc::new(Semaphore::new(RELAY_LIMIT)),
            cleanup: Arc::new(Semaphore::new(CLEANUP_LIMIT)),
            joins: Pending::new(JOIN_LIMIT, MAX_WAITING_JOINS),
            voices: std::sync::Mutex::default(),
            interactions: Interactions::default(),
            bot_messages: tokio::sync::Mutex::new(()),
            picker_retry: std::sync::Mutex::default(),
            reactions: Default::default(),
            started_at: crate::db::now(),
            started: OnceLock::new(),
        }))
    }

    pub async fn react(
        &self,
        reaction: Reaction,
        channel: Id<ChannelMarker>,
        message: Id<MessageMarker>,
    ) {
        let (kind, emoji, per_minute) = match reaction {
            Reaction::Busy => ("busy", "⏳", 20),
            Reaction::Relayed => ("relayed", "✅", 60),
        };
        if !self.reactions[reaction as usize].take(per_minute) {
            metrics::counter!("reactions_skipped_total", "kind" => kind).increment(1);
            return;
        }
        let emoji =
            twilight_http::request::channel::reaction::RequestReactionType::Unicode { name: emoji };
        if let Err(error) = self
            .http
            .create_reaction(channel, message, &emoji)
            .await
        {
            metrics::counter!("reactions_failed_total", "kind" => kind).increment(1);
            tracing::debug!(%error, kind, "reaction failed");
        }
    }

    pub fn guild(&self, guild: Id<GuildMarker>) -> Arc<GuildConfig> {
        self.guilds.get(guild)
    }

    pub fn guild_name(&self, guild: Id<GuildMarker>) -> String {
        self.cache
            .guild(guild)
            .map(|g| g.name().to_owned())
            .unwrap_or_default()
    }

    pub fn guild_icon(&self, guild: Id<GuildMarker>) -> Option<String> {
        let cached = self.cache.guild(guild)?;
        let icon = cached.icon()?;
        Some(format!(
            "https://cdn.discordapp.com/icons/{}/{icon}.png?size=128",
            guild.get()
        ))
    }

    pub fn bot_top_position(&self, guild: Id<GuildMarker>) -> Option<i64> {
        let member = self.cache.member(guild, self.bot_id)?;
        Some(
            member
                .roles()
                .iter()
                .filter_map(|r| {
                    self.cache
                        .role(*r)
                        .map(|role| role.position)
                })
                .max()
                .unwrap_or(0),
        )
    }

    pub fn can_manage_role(
        &self,
        guild: Id<GuildMarker>,
        role: Id<twilight_model::id::marker::RoleMarker>,
    ) -> bool {
        if role.get() == guild.get() {
            return false;
        }
        // A role deleted while offline can linger in the cache's role map.
        if self
            .cache
            .guild_roles(guild)
            .is_some_and(|roles| !roles.contains(&role))
        {
            return false;
        }
        let Some(cached) = self.cache.role(role) else {
            return self.cache.guild_roles(guild).is_none();
        };
        if cached.managed {
            return false;
        }
        self.bot_top_position(guild)
            .is_none_or(|top| cached.position < top)
    }

    pub fn spawn<F>(self: &Arc<Self>, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.tasks.spawn(future);
    }

    /// Outside the event slots, so a flooded channel's rate limit can't stall moderation.
    /// Dropped when full.
    pub fn spawn_outbound<F>(self: &Arc<Self>, kind: &'static str, future: F) -> bool
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_limited(&self.outbound, kind, OUTBOUND_TIMEOUT, future)
    }

    /// Relay has its own smaller pool: each copy holds its card while it waits on rate limits.
    pub fn spawn_relay<F>(self: &Arc<Self>, future: F) -> bool
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_limited(
            &self.relay_posts,
            "relay",
            OUTBOUND_TIMEOUT,
            future,
        )
    }

    /// Deletes run one at a time per channel, so restricted accounts posting fast would fill the
    /// event slots. Skipped when full, since the member is restricted anyway.
    pub fn spawn_cleanup<F>(self: &Arc<Self>, kind: &'static str, future: F) -> bool
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_limited(&self.cleanup, kind, CLEANUP_TIMEOUT, future)
    }

    fn spawn_limited<F>(
        self: &Arc<Self>,
        pool: &Arc<Semaphore>,
        kind: &'static str,
        timeout: Duration,
        future: F,
    ) -> bool
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let Ok(permit) = Arc::clone(pool).try_acquire_owned() else {
            metrics::counter!("outbound_dropped_total", "kind" => kind).increment(1);
            return false;
        };
        // Boxed, or the task would store the future twice.
        let timed = Box::pin(tokio::time::timeout(timeout, future));
        self.spawn(async move {
            let _permit = permit;
            if timed.await.is_err() {
                metrics::counter!("outbound_timeouts_total", "kind" => kind).increment(1);
            }
        });
        true
    }
}

const OUTBOUND_LIMIT: usize = 128;
const RELAY_LIMIT: usize = 64;
const OUTBOUND_TIMEOUT: Duration = Duration::from_secs(120);
const CLEANUP_LIMIT: usize = 64;
const JOIN_LIMIT: usize = 16;
const MAX_WAITING_JOINS: usize = 16384;
const INTERACTION_LIMIT: usize = 64;
const INTERACTIONS_PER_MEMBER: usize = 4;
const STAFF_INTERACTIONS: usize = 16;

/// Drained by a few on-demand workers, so a waiting item costs its size, not a task.
pub struct Pending<T> {
    state: std::sync::Mutex<PendingState<T>>,
    max_workers: usize,
    cap: usize,
}

struct PendingState<T> {
    queue: std::collections::VecDeque<T>,
    workers: usize,
}

impl<T> Pending<T> {
    pub fn new(max_workers: usize, cap: usize) -> Self {
        Self {
            state: std::sync::Mutex::new(PendingState {
                queue: std::collections::VecDeque::new(),
                workers: 0,
            }),
            max_workers,
            cap,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, PendingState<T>> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Returns workers to start and items dropped. Urgent items go first and are never dropped.
    pub fn push(&self, items: impl IntoIterator<Item = T>, urgent: bool) -> (usize, usize) {
        let mut state = self.state();
        let mut dropped = 0;
        if urgent {
            let items: Vec<T> = items.into_iter().collect();
            for item in items.into_iter().rev() {
                state.queue.push_front(item);
            }
        } else {
            for item in items {
                if state.queue.len() < self.cap {
                    state.queue.push_back(item);
                } else {
                    dropped += 1;
                }
            }
        }
        let wanted = state.queue.len().min(self.max_workers);
        let start = wanted.saturating_sub(state.workers);
        state.workers += start;
        (start, dropped)
    }

    /// Counts itself out when the queue empties or on drop, so a lost worker never blocks new ones.
    pub fn worker(&self) -> Worker<'_, T> {
        Worker { pending: self, counted_out: false }
    }

    #[cfg(test)]
    pub fn len(&self) -> (usize, usize) {
        let state = self.state();
        (state.queue.len(), state.workers)
    }
}

pub struct Worker<'a, T> {
    pending: &'a Pending<T>,
    counted_out: bool,
}

impl<T> Worker<'_, T> {
    /// Counted out under the lock that saw the queue empty, so a later push starts a new worker.
    pub fn take(&mut self) -> Option<T> {
        if self.counted_out {
            return None;
        }
        let mut state = self.pending.state();
        let next = state.queue.pop_front();
        if next.is_none() {
            state.workers -= 1;
            self.counted_out = true;
        }
        next
    }
}

impl<T> Drop for Worker<'_, T> {
    fn drop(&mut self) {
        if !self.counted_out {
            self.pending.state().workers -= 1;
        }
    }
}

/// Per-member cap, plus separate places for staff so members can't block moderation.
pub struct Interactions {
    permits: Arc<Semaphore>,
    staff: Arc<Semaphore>,
    members: Arc<std::sync::Mutex<HashMap<Option<Id<UserMarker>>, usize>>>,
}

impl Default for Interactions {
    fn default() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(INTERACTION_LIMIT)),
            staff: Arc::new(Semaphore::new(STAFF_INTERACTIONS)),
            members: Arc::default(),
        }
    }
}

pub enum InteractionRefused {
    Full,
    Member,
}

impl Interactions {
    pub fn start(
        &self,
        member: Option<Id<UserMarker>>,
        staff: bool,
    ) -> Result<InteractionSlot, InteractionRefused> {
        let mut members = self
            .members
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if members
            .get(&member)
            .copied()
            .unwrap_or(0)
            >= INTERACTIONS_PER_MEMBER
        {
            return Err(InteractionRefused::Member);
        }
        let permit = Arc::clone(&self.permits)
            .try_acquire_owned()
            .or_else(|error| match staff {
                true => Arc::clone(&self.staff).try_acquire_owned(),
                false => Err(error),
            })
            .map_err(|_| InteractionRefused::Full)?;
        *members.entry(member).or_default() += 1;
        Ok(InteractionSlot { _permit: permit, member, members: Arc::clone(&self.members) })
    }
}

pub struct InteractionSlot {
    _permit: tokio::sync::OwnedSemaphorePermit,
    member: Option<Id<UserMarker>>,
    members: Arc<std::sync::Mutex<HashMap<Option<Id<UserMarker>>, usize>>>,
}

impl Drop for InteractionSlot {
    fn drop(&mut self) {
        let mut members = self
            .members
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(held) = members.get_mut(&self.member) {
            *held -= 1;
            if *held == 0 {
                members.remove(&self.member);
            }
        }
    }
}
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);

/// Retries until success, a 4xx or shutdown: after a reboot the network may come up late.
pub async fn retry<T, F, Fut>(
    shutdown: &CancellationToken,
    what: &'static str,
    mut call: F,
) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    let mut delay = Duration::from_secs(1);
    let mut failures = 0u32;
    loop {
        let result = tokio::select! {
            () = shutdown.cancelled() => anyhow::bail!("shutdown requested while waiting to {what}"),
            result = call() => result,
        };
        match result {
            Ok(value) => {
                if failures > 0 {
                    tracing::info!(what, failures, "succeeded after retrying");
                }
                return Ok(value);
            }
            Err(error) if is_rejected(&error) => return Err(error),
            Err(error) if failures == 0 => {
                tracing::warn!(what, %error, "failed; retrying until Discord is reachable");
            }
            Err(error) => tracing::debug!(what, failures, %error, "still failing"),
        }
        failures += 1;
        tokio::select! {
            () = shutdown.cancelled() => anyhow::bail!("shutdown requested while waiting to {what}"),
            () = tokio::time::sleep(delay) => {}
        }
        delay = (delay * 2).min(Duration::from_secs(30));
    }
}

fn is_rejected(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<twilight_http::Error>()
        .and_then(crate::discord::errors::status)
        .is_some_and(|status| (400..500).contains(&status) && status != 429)
}

pub fn avatar_url(user: &twilight_model::user::User) -> String {
    match &user.avatar {
        Some(hash) => format!(
            "https://cdn.discordapp.com/avatars/{}/{hash}.png?size=128",
            user.id.get()
        ),
        None => format!(
            "https://cdn.discordapp.com/embed/avatars/{}.png",
            (user.id.get() >> 22) % 6
        ),
    }
}

pub fn display_name(user: &twilight_model::user::User) -> &str {
    user.global_name
        .as_deref()
        .unwrap_or(&user.name)
}

#[cfg(test)]
pub(crate) async fn test_app() -> Arc<App> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = Config {
        discord_token: "test".into(),
        database_path: ":memory:".into(),
        http_addr: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        ai: crate::config::AiConfig::from_env().expect("ai config"),
        scam: crate::config::ScamConfig::from_env().expect("scam config"),
    };
    let db = Db::open_in_memory().expect("db");
    let guilds = Guilds::load(db.clone())
        .await
        .expect("guilds");
    let web = reqwest::Client::new();
    Arc::new(App {
        moderation: Moderation::new(&config.scam).expect("moderation"),
        ai: Ai::new(web.clone(), &config.ai),
        images: crate::image_worker::ImageWorker::new("images"),
        warframe: Warframe::default(),
        relay: Relay::default(),
        health: Health::default(),
        http: twilight_http::Client::new("test".into()),
        callbacks: twilight_http::Client::new("test".into()),
        cache: DefaultInMemoryCache::new(),
        application_id: Id::new(1),
        bot_id: Id::new(2),
        db,
        guilds,
        web,
        tasks: TaskTracker::new(),
        shutdown: CancellationToken::new(),
        event_permits: Semaphore::new(64),
        event_backlog: Arc::new(Semaphore::new(
            crate::gateway::EVENT_BACKLOG_KIB as usize,
        )),
        outbound: Arc::new(Semaphore::new(OUTBOUND_LIMIT)),
        relay_posts: Arc::new(Semaphore::new(RELAY_LIMIT)),
        cleanup: Arc::new(Semaphore::new(CLEANUP_LIMIT)),
        joins: Pending::new(JOIN_LIMIT, MAX_WAITING_JOINS),
        voices: std::sync::Mutex::default(),
        interactions: Interactions::default(),
        bot_messages: tokio::sync::Mutex::new(()),
        picker_retry: std::sync::Mutex::default(),
        reactions: Default::default(),
        started_at: 0,
        started: OnceLock::new(),
        config,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_budget_refuses_past_its_rate() {
        let budget = super::Budget::default();
        assert!((0..3).all(|_| budget.take(3)));
        assert!(!budget.take(3));
    }

    #[test]
    fn interactions_are_shared_out() {
        let interactions = super::Interactions::default();
        let id = |n: u64| Some(twilight_model::id::Id::new(n));
        let held: Vec<_> = (0..super::INTERACTIONS_PER_MEMBER)
            .map(|_| {
                interactions
                    .start(id(7), true)
                    .ok()
                    .unwrap()
            })
            .collect();
        assert!(matches!(
            interactions.start(id(7), true),
            Err(super::InteractionRefused::Member)
        ));
        drop(held);
        let others: Vec<_> = (0..super::INTERACTION_LIMIT as u64)
            .map(|n| {
                interactions
                    .start(id(100 + n), false)
                    .ok()
                    .unwrap()
            })
            .collect();
        assert!(matches!(
            interactions.start(id(7), false),
            Err(super::InteractionRefused::Full)
        ));
        let staff: Vec<_> = (0..super::STAFF_INTERACTIONS as u64)
            .map(|n| {
                interactions
                    .start(id(1000 + n), true)
                    .ok()
                    .unwrap()
            })
            .collect();
        assert!(matches!(
            interactions.start(id(8), true),
            Err(super::InteractionRefused::Full)
        ));
        assert_eq!(
            interactions
                .members
                .lock()
                .unwrap()
                .len(),
            super::INTERACTION_LIMIT + super::STAFF_INTERACTIONS
        );
        drop((others, staff));
        assert!(
            interactions
                .members
                .lock()
                .unwrap()
                .is_empty()
        );
    }

    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn retry_backs_off_until_the_call_succeeds() {
        let calls = AtomicU32::new(0);
        let started = tokio::time::Instant::now();
        let value = retry(&CancellationToken::new(), "test", || async {
            if calls.fetch_add(1, Ordering::Relaxed) < 3 {
                anyhow::bail!("network unreachable");
            }
            Ok(42)
        })
        .await
        .unwrap();
        assert_eq!(value, 42);
        assert_eq!(calls.load(Ordering::Relaxed), 4);
        assert_eq!(started.elapsed(), Duration::from_secs(1 + 2 + 4));
    }

    #[tokio::test(start_paused = true)]
    async fn retry_stops_when_shutdown_is_requested() {
        let shutdown = CancellationToken::new();
        let cancel = shutdown.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(90)).await;
            cancel.cancel();
        });
        let started = tokio::time::Instant::now();
        let result: anyhow::Result<()> = retry(&shutdown, "test", || async {
            anyhow::bail!("dns lookup failed")
        })
        .await;
        assert!(result.is_err());
        assert_eq!(started.elapsed(), Duration::from_secs(90));
    }
}
