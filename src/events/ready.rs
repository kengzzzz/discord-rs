use twilight_model::gateway::payload::incoming::Ready;

use crate::{
    context::Context,
    services::{
        build::BuildService, health::HealthService, market::MarketService,
        notification::NotificationService, role_message, status::StatusService,
    },
};
use once_cell::sync::Lazy;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

static INIT: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

pub async fn handle(ctx: Arc<Context>, event: Ready) {
    HealthService::set_ready(true);
    HealthService::set_discord(true);

    let role_ctx = ctx.clone();
    tokio::spawn(async move {
        for guild in event.guilds {
            role_message::handler::ensure_message(&role_ctx, guild.id).await;
        }
    });

    if !INIT.swap(true, Ordering::Relaxed) {
        StatusService::spawn(&ctx);

        let build_ctx = ctx.clone();
        tokio::spawn(async move {
            BuildService::init(build_ctx).await;
        });

        let market_ctx = ctx.clone();
        tokio::spawn(async move {
            MarketService::init(market_ctx).await;
        });

        NotificationService::spawn(ctx);
    }

    tracing::info!(
        user = %event.user.name,
        id = %event.user.id,
        "Logged in"
    );
}
