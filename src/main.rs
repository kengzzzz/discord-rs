use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use twilight_gateway::{Intents, Shard, ShardId};

use discord_bot::{
    bot::Bot,
    configs::discord::DISCORD_CONFIGS,
    context::ContextBuilder,
    observability::server::{ServerConfig, start_server},
    services::shutdown,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .unwrap();
    tracing_subscriber::fmt::init();

    let shutdown = CancellationToken::new();
    shutdown::set_token(shutdown.clone());

    let ctx = Arc::new(ContextBuilder::new().build().await?);

    let shard = Shard::new(
        ShardId::ONE,
        DISCORD_CONFIGS.discord_token.clone(),
        Intents::GUILDS
            | Intents::GUILD_MEMBERS
            | Intents::GUILD_MESSAGES
            | Intents::GUILD_MESSAGE_REACTIONS
            | Intents::MESSAGE_CONTENT,
    );

    let shutdown_clone = shutdown.clone();
    start_server(ServerConfig { shutdown: async move { shutdown_clone.cancelled().await } });

    let bot = Bot::new(ctx, shard).await?;

    let ctrl = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("install CTRL+C handler");
        ctrl.cancel();
    });

    bot.run(shutdown).await
}
