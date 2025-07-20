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
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_interactions::command::CreateCommand;
use twilight_model::guild::Permissions;

const EVENT_CONCURRENCY: usize = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PriorityClass {
    High,
    Normal,
    Low,
    Ignore,
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
    let sem = Arc::new(Semaphore::new(EVENT_CONCURRENCY));

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
                    PriorityClass::High | PriorityClass::Normal | PriorityClass::Low=> {
                        let permit = sem.clone().acquire_owned().await.unwrap();
                        let ctx_clone = ctx.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            dispatch_event(ctx_clone, event).await
                        });
                    }
                }
            }
        }
    }

    HealthService::set_discord(false);
    HealthService::set_ready(false);

    Ok(())
}

async fn dispatch_event(ctx: Arc<Context>, event: Event) {
    match event {
        Event::MessageCreate(r#box) => message_create::handle(ctx, (*r#box).0).await,
        Event::InteractionCreate(r#box) => interaction_create::handle(ctx, (*r#box).0).await,
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
