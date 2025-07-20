use discord_bot::{
    commands::{
        admin::AdminCommand, ai::AiCommand, help::HelpCommand, intro::IntroCommand,
        ping::PingCommand, verify::VerifyCommand, warframe::WarframeCommand,
    },
    configs::discord::DISCORD_CONFIGS,
    context::{Context, ContextBuilder},
    events::{
        interaction_create, member_add, member_remove, message_create, message_delete,
        reaction_add, reaction_remove, ready,
    },
    services::{health::HealthService, latency::LatencyService, shutdown},
};
use metrics::Histogram;
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_interactions::command::CreateCommand;
use twilight_model::guild::Permissions;

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

static HIGH_QUEUE_WAIT: Lazy<Histogram> =
    Lazy::new(|| metrics::histogram!("bot_queue_wait_seconds", "priority" => "high"));
static NORMAL_QUEUE_WAIT: Lazy<Histogram> =
    Lazy::new(|| metrics::histogram!("bot_queue_wait_seconds", "priority" => "normal"));
static LOW_QUEUE_WAIT: Lazy<Histogram> =
    Lazy::new(|| metrics::histogram!("bot_queue_wait_seconds", "priority" => "low"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PriorityClass {
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

struct EnqueuedEvent {
    event: Event,
    enqueue_at: std::time::Instant,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .unwrap();

    tracing_subscriber::fmt::init();
    let token = DISCORD_CONFIGS.discord_token.clone();

    let mut shard = Shard::new(
        ShardId::ONE,
        token.clone(),
        Intents::GUILDS
            | Intents::GUILD_MEMBERS
            | Intents::GUILD_MESSAGES
            | Intents::GUILD_MESSAGE_REACTIONS
            | Intents::MESSAGE_CONTENT,
    );

    let shutdown_token = CancellationToken::new();
    shutdown::set_token(shutdown_token.clone());

    let ctx = Arc::new(ContextBuilder::new().build().await?);
    let high_sem = Arc::new(Semaphore::new(HIGH_PERMITS));
    let normal_sem = Arc::new(Semaphore::new(NORMAL_PERMITS));
    let low_sem = Arc::new(Semaphore::new(LOW_PERMITS));
    let (high_tx, mut high_rx) = mpsc::channel::<EnqueuedEvent>(HIGH_QUEUE_CAP);
    let (low_tx, mut low_rx) = mpsc::channel::<EnqueuedEvent>(LOW_QUEUE_CAP);
    let (normal_tx, mut normal_rx) = mpsc::channel::<EnqueuedEvent>(NORMAL_QUEUE_CAP);

    let shutdown_clone = shutdown_token.clone();
    HealthService::spawn(async move {
        shutdown_clone.cancelled().await;
    });

    let token_clone = shutdown_token.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install CTRL+C handler");
        token_clone.cancel();
    });

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

    {
        let ctx_high = ctx.clone();
        tokio::spawn(async move {
            while let Some(EnqueuedEvent { event, enqueue_at }) = high_rx.recv().await {
                let queue_wait_seconds = enqueue_at.elapsed().as_secs_f64();
                HIGH_QUEUE_WAIT.record(queue_wait_seconds);

                let permit = match high_sem.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => {
                        tracing::info!("high semaphore closed, exit worker");
                        break;
                    }
                };
                let ctx2 = ctx_high.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    handle_interaction_fast(ctx2, event).await
                });
            }
        });
    }

    {
        let ctx_norm = ctx.clone();
        tokio::spawn(async move {
            while let Some(EnqueuedEvent { event, enqueue_at }) = normal_rx.recv().await {
                let queue_wait_seconds = enqueue_at.elapsed().as_secs_f64();
                NORMAL_QUEUE_WAIT.record(queue_wait_seconds);

                let permit = match normal_sem.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => {
                        tracing::info!("normal semaphore closed, exit worker");
                        break;
                    }
                };
                let ctx2 = ctx_norm.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    dispatch_event(ctx2, event).await
                });
            }
        });
    }

    {
        let ctx_low = ctx.clone();
        tokio::spawn(async move {
            while let Some(EnqueuedEvent { event, enqueue_at }) = low_rx.recv().await {
                let queue_wait_seconds = enqueue_at.elapsed().as_secs_f64();
                LOW_QUEUE_WAIT.record(queue_wait_seconds);

                let permit = match low_sem.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => {
                        tracing::info!("low semaphore closed, exit worker");
                        break;
                    }
                };
                let ctx2 = ctx_low.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    dispatch_event(ctx2, event).await
                });
            }
        });
    }

    let mut failure_count = 0usize;

    loop {
        tokio::select! {
             _ = shutdown_token.cancelled() => {
                break;
            }
            item = shard.next_event(EventTypeFlags::all()) => {
                let Some(item) = item else { break };
                let Ok(event) = item else {
                    tracing::warn!(source = ?item.unwrap_err(), "error receiving event");
                    failure_count += 1;
                    if failure_count >= 5 {
                        HealthService::set_discord(false);
                    }
                    continue;
                };

                failure_count = 0;
                ctx.cache.update(&event);
                LatencyService::update(shard.latency().average());
                HealthService::set_discord(shard.state().is_identified());

                match classify_priority(&event) {
                    PriorityClass::Ignore => continue,
                    priority @ PriorityClass::High => {
                        let start = std::time::Instant::now();
                        let ev_type = event_type(&event);
                        if let Err(e) = high_tx.send(EnqueuedEvent { event, enqueue_at: start }).await {
                            tracing::warn!(error=?e, "high queue closed");
                            break;
                        } else {
                            let blocked = start.elapsed().as_secs_f64();
                            HIGH_ENQ_BLOCK.record(blocked);
                            metrics::counter!("bot_events_total",
                                "priority"=>priority.as_str(),
                                "event_type"=>ev_type,
                                "result"=>"enqueued").increment(1);
                        }
                    }
                    priority @ PriorityClass::Normal => {
                        let start = std::time::Instant::now();
                        let ev_type = event_type(&event);
                        if let Err(e) = normal_tx.send(EnqueuedEvent { event, enqueue_at: start }).await {
                            tracing::warn!(error=?e, "normal queue closed");
                            break;
                        } else {
                            let blocked = start.elapsed().as_secs_f64();
                            NORMAL_ENQ_BLOCK.record(blocked);
                            metrics::counter!("bot_events_total",
                                "priority"=>priority.as_str(),
                                "event_type"=>ev_type,
                                "result"=>"enqueued").increment(1);
                        }
                    }
                    priority @ PriorityClass::Low => {
                        let start = std::time::Instant::now();
                        let ev_type = event_type(&event);
                        if let Err(e) = low_tx.send(EnqueuedEvent { event, enqueue_at: start }).await {
                            tracing::warn!(error=?e, "low queue closed");
                            break;
                        } else {
                            let blocked = start.elapsed().as_secs_f64();
                            LOW_ENQ_BLOCK.record(blocked);
                            metrics::counter!("bot_events_total",
                                "priority"=>priority.as_str(),
                                "event_type"=>ev_type,
                                "result"=>"enqueued").increment(1);
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

async fn handle_interaction_fast(ctx: Arc<Context>, event: Event) {
    let Event::InteractionCreate(r#box) = event else {
        return;
    };
    interaction_create::handle(ctx, (*r#box).0).await
}

async fn dispatch_event(ctx: Arc<Context>, event: Event) {
    match event {
        Event::MessageCreate(r#box) => message_create::handle(ctx, (*r#box).0).await,
        Event::Ready(r#box) => ready::handle(ctx, *r#box).await,
        Event::MemberAdd(r#box) => member_add::handle(ctx, *r#box).await,
        Event::MemberRemove(event) => member_remove::handle(ctx, event).await,
        Event::ReactionAdd(r#box) => reaction_add::handle(ctx, *r#box).await,
        Event::ReactionRemove(r#box) => reaction_remove::handle(ctx, *r#box).await,
        Event::MessageDelete(event) => message_delete::handle_single(ctx, event).await,
        Event::MessageDeleteBulk(event) => message_delete::handle_bulk(ctx, event).await,
        _ => {}
    }
}
