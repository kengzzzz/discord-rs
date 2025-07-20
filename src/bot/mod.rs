pub mod worker;

use metrics::Histogram;
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use twilight_gateway::StreamExt;
use twilight_gateway::{Event, EventTypeFlags, Shard};
use twilight_interactions::command::CreateCommand;
use twilight_model::guild::Permissions;

use crate::{
    commands::{
        admin::AdminCommand, ai::AiCommand, help::HelpCommand, intro::IntroCommand,
        ping::PingCommand, verify::VerifyCommand, warframe::WarframeCommand,
    },
    context::Context,
    services::{health::HealthService, latency::LatencyService},
};

use worker::{EnqueuedEvent, Worker};

const HIGH_QUEUE_CAP: usize = 64;
const HIGH_PERMITS: usize = 8;
const NORMAL_QUEUE_CAP: usize = 256;
const NORMAL_PERMITS: usize = 32;
const LOW_QUEUE_CAP: usize = 24;
const LOW_PERMITS: usize = 8;

static HIGH_ENQ_BLOCK: Lazy<Histogram> =
    Lazy::new(|| metrics::histogram!("bot_queue_enqueue_block_seconds", "priority" => "high"));
static NORMAL_ENQ_BLOCK: Lazy<Histogram> =
    Lazy::new(|| metrics::histogram!("bot_queue_enqueue_block_seconds", "priority" => "normal"));
static LOW_ENQ_BLOCK: Lazy<Histogram> =
    Lazy::new(|| metrics::histogram!("bot_queue_enqueue_block_seconds", "priority" => "low"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PriorityClass {
    High,
    Normal,
    Low,
    Ignore,
}

impl PriorityClass {
    fn as_str(&self) -> &'static str {
        match self {
            PriorityClass::High => "high",
            PriorityClass::Normal => "normal",
            PriorityClass::Low => "low",
            PriorityClass::Ignore => "ignore",
        }
    }
}

fn event_type(e: &Event) -> &'static str {
    match e {
        Event::InteractionCreate(_) => "interaction",
        Event::MessageCreate(_) => "message_create",
        Event::ReactionAdd(_) => "reaction_add",
        Event::ReactionRemove(_) => "reaction_remove",
        Event::MemberAdd(_) => "member_add",
        Event::MemberRemove(_) => "member_remove",
        Event::MessageDelete(_) => "message_delete",
        Event::MessageDeleteBulk(_) => "message_delete_bulk",
        Event::Ready(_) => "ready",
        _ => "other",
    }
}

fn classify_priority(e: &Event) -> PriorityClass {
    if matches!(e, Event::InteractionCreate(_)) {
        PriorityClass::High
    } else if matches!(
        e,
        Event::MessageCreate(_)
            | Event::ReactionAdd(_)
            | Event::ReactionRemove(_)
            | Event::MemberAdd(_)
    ) {
        PriorityClass::Normal
    } else if matches!(
        e,
        Event::Ready(_)
            | Event::MemberRemove(_)
            | Event::MessageDelete(_)
            | Event::MessageDeleteBulk(_)
    ) {
        PriorityClass::Low
    } else {
        PriorityClass::Ignore
    }
}

pub struct Bot {
    shard: Shard,
    ctx: Arc<Context>,
    high_tx: mpsc::Sender<EnqueuedEvent>,
    normal_tx: mpsc::Sender<EnqueuedEvent>,
    low_tx: mpsc::Sender<EnqueuedEvent>,
}

impl Bot {
    pub async fn new(ctx: Arc<Context>, shard: Shard) -> anyhow::Result<Self> {
        let mut admin_commands = AdminCommand::create_command();
        admin_commands.default_member_permissions = Some(Permissions::ADMINISTRATOR);
        let verify_command = VerifyCommand::create_command();
        let warframe_command = WarframeCommand::create_command();
        let ai_command = AiCommand::create_command();
        let ping_command = PingCommand::create_command();
        let help_command = HelpCommand::create_command();
        let intro_command = IntroCommand::create_command();

        let commands = [
            admin_commands.into(),
            verify_command.into(),
            warframe_command.into(),
            ai_command.into(),
            ping_command.into(),
            intro_command.into(),
            help_command.into(),
        ];

        let application = ctx
            .http
            .current_user_application()
            .await?
            .model()
            .await?;
        let interaction_client = ctx.http.interaction(application.id);
        interaction_client
            .set_global_commands(&commands)
            .await?;

        let high_sem = Arc::new(Semaphore::new(HIGH_PERMITS));
        let normal_sem = Arc::new(Semaphore::new(NORMAL_PERMITS));
        let low_sem = Arc::new(Semaphore::new(LOW_PERMITS));

        let (high_tx, high_rx) = mpsc::channel(HIGH_QUEUE_CAP);
        let (normal_tx, normal_rx) = mpsc::channel(NORMAL_QUEUE_CAP);
        let (low_tx, low_rx) = mpsc::channel(LOW_QUEUE_CAP);

        Worker::spawn(
            ctx.clone(),
            high_sem,
            high_rx,
            PriorityClass::High,
            |ctx, event| async move { super::dispatch::handle_interaction_fast(ctx, event).await },
        );
        Worker::spawn(
            ctx.clone(),
            normal_sem,
            normal_rx,
            PriorityClass::Normal,
            |ctx, event| async move { super::dispatch::dispatch_event(ctx, event).await },
        );
        Worker::spawn(
            ctx.clone(),
            low_sem,
            low_rx,
            PriorityClass::Low,
            |ctx, event| async move { super::dispatch::dispatch_event(ctx, event).await },
        );

        Ok(Self { shard, ctx, high_tx, normal_tx, low_tx })
    }

    pub async fn run(mut self, shutdown: CancellationToken) -> anyhow::Result<()> {
        let mut failure_count = 0usize;

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                item = self.shard.next_event(EventTypeFlags::all()) => {
                    let Some(item) = item else { break };
                    let Ok(event) = item else {
                        tracing::warn!(source=?item.unwrap_err(), "error receiving event");
                        failure_count += 1;
                        if failure_count >= 5 {
                            HealthService::set_discord(false);
                        }
                        continue;
                    };

                    failure_count = 0;
                    self.ctx.cache.update(&event);
                    LatencyService::update(self.shard.latency().average());
                    HealthService::set_discord(self.shard.state().is_identified());

                    match classify_priority(&event) {
                        PriorityClass::Ignore => continue,
                        priority @ PriorityClass::High => {
                            let start = std::time::Instant::now();
                            let ev_type = event_type(&event);
                            if let Err(e) = self.high_tx.send(EnqueuedEvent { event, enqueue_at: start }).await {
                                tracing::warn!(error=?e, "high queue closed");
                                break;
                            } else {
                                let blocked = start.elapsed().as_secs_f64();
                                HIGH_ENQ_BLOCK.record(blocked);
                                metrics::counter!("bot_events_total", "priority" => priority.as_str(), "event_type" => ev_type, "result" => "enqueued").increment(1);
                            }
                        }
                        priority @ PriorityClass::Normal => {
                            let start = std::time::Instant::now();
                            let ev_type = event_type(&event);
                            if let Err(e) = self.normal_tx.send(EnqueuedEvent { event, enqueue_at: start }).await {
                                tracing::warn!(error=?e, "normal queue closed");
                                break;
                            } else {
                                let blocked = start.elapsed().as_secs_f64();
                                NORMAL_ENQ_BLOCK.record(blocked);
                                metrics::counter!("bot_events_total", "priority" => priority.as_str(), "event_type" => ev_type, "result" => "enqueued").increment(1);
                            }
                        }
                        priority @ PriorityClass::Low => {
                            let start = std::time::Instant::now();
                            let ev_type = event_type(&event);
                            if let Err(e) = self.low_tx.send(EnqueuedEvent { event, enqueue_at: start }).await {
                                tracing::warn!(error=?e, "low queue closed");
                                break;
                            } else {
                                let blocked = start.elapsed().as_secs_f64();
                                LOW_ENQ_BLOCK.record(blocked);
                                metrics::counter!("bot_events_total", "priority" => priority.as_str(), "event_type" => ev_type, "result" => "enqueued").increment(1);
                            }
                        }
                    }
                }
            }
        }

        HealthService::set_discord(false);
        HealthService::set_ready(false);

        Ok(())
    }
}
