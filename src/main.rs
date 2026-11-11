use std::{sync::Arc, time::Duration};

use tokio_util::sync::CancellationToken;
use tracing_subscriber::{EnvFilter, fmt};
use twilight_gateway::{Intents, Shard, ShardId};

use discord_bot::{
    app::{self, App},
    commands,
    config::Config,
    gateway, health,
};

const DRAIN: Duration = Duration::from_secs(15);

fn main() -> anyhow::Result<()> {
    // The worker's stdout carries its replies, so logs go to stderr.
    if std::env::args().nth(1).as_deref() == Some(discord_bot::image_worker::ARG) {
        fmt()
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .init();
        return discord_bot::image_worker::run();
    }
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("failed to install rustls crypto provider"))?;
    fmt()
        .with_ansi(std::io::IsTerminal::is_terminal(
            &std::io::stdout(),
        ))
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                EnvFilter::new("info,twilight_gateway=warn,twilight_http_ratelimiting=warn")
            }),
        )
        .init();
    // A panic ends only its task, so log it.
    std::panic::set_hook(Box::new(|info| {
        metrics::counter!("panics_total").increment(1);
        tracing::error!(%info, "panic");
    }));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(run());
    runtime.shutdown_timeout(Duration::from_secs(2));
    result
}

async fn run() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let metrics = health::install_metrics()?;

    let shutdown = CancellationToken::new();
    let signal = shutdown_signal()?;
    let signals = shutdown.clone();
    tokio::spawn(async move {
        signal.await;
        tracing::info!("shutdown requested");
        signals.cancel();
    });

    let app = match App::build(config, shutdown.clone()).await {
        Ok(app) => app,
        Err(_) if shutdown.is_cancelled() => {
            tracing::info!("stopped before connecting");
            return Ok(());
        }
        Err(error) => return Err(error),
    };

    let sync = Arc::clone(&app);
    app.spawn(async move {
        let commands = commands::all();
        let result = app::retry(
            &sync.shutdown,
            "register application commands",
            || async {
                sync.http
                    .interaction(sync.application_id)
                    .set_global_commands(&commands)
                    .await?;
                Ok(())
            },
        )
        .await;
        match result {
            Ok(()) => tracing::info!(
                count = commands.len(),
                "application commands registered"
            ),
            Err(_) if sync.shutdown.is_cancelled() => {}
            Err(error) => tracing::error!(%error, "application commands were not registered"),
        }
    });

    let health_app = Arc::clone(&app);
    let view = health::HealthView {
        health: Arc::new(move || health_app.health.is_ready()),
        db: app.db.clone(),
    };
    let server = tokio::spawn({
        let (addr, shutdown) = (app.config.http_addr, app.shutdown.clone());
        async move {
            if let Err(error) = health::serve(addr, view, metrics, shutdown).await {
                tracing::error!(%addr, %error, "health server stopped");
            }
        }
    });

    let shard = Shard::new(
        ShardId::ONE,
        app.config.discord_token.clone(),
        Intents::GUILDS
            | Intents::GUILD_MEMBERS
            | Intents::GUILD_MESSAGES
            | Intents::GUILD_VOICE_STATES
            | Intents::MESSAGE_CONTENT,
    );
    let outcome = gateway::run(Arc::clone(&app), shard).await;

    app.shutdown.cancel();
    app.health.set_ready(false);
    app.tasks.close();
    let started = std::time::Instant::now();
    if tokio::time::timeout(DRAIN, app.tasks.wait())
        .await
        .is_err()
    {
        tracing::warn!("timed out waiting for in-flight tasks");
    }
    // A scraper can keep a connection open and block graceful shutdown.
    if tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .is_err()
    {
        tracing::warn!("timed out waiting for the health server");
    }
    match tokio::time::timeout(
        Duration::from_secs(5),
        app.db
            .call(|c| c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "final WAL checkpoint failed"),
        Err(_) => tracing::warn!("final WAL checkpoint timed out"),
    }
    tracing::info!(
        drain_ms = started.elapsed().as_millis() as u64,
        "stopped"
    );
    outcome
}

/// Registers handlers at once: as PID 1, a SIGTERM with no handler is ignored.
fn shutdown_signal() -> anyhow::Result<impl Future<Output = ()>> {
    #[cfg(unix)]
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    Ok(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
        #[cfg(not(unix))]
        let _ = ctrl_c.await;
    })
}
