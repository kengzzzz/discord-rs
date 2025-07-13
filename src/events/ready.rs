use twilight_model::gateway::payload::incoming::Ready;

use crate::{
    context::Context,
    services::{
        build::BuildService, health::HealthService, market::MarketService,
        notification::NotificationService, role_message::RoleMessageService, status::StatusService,
    },
};
use std::sync::Arc;

pub async fn handle(ctx: Arc<Context>, event: Ready) {
    HealthService::set_ready(true);
    HealthService::set_discord(true);

    let role_ctx = ctx.clone();
    tokio::spawn(async move {
        for guild in event.guilds {
            RoleMessageService::ensure_message(role_ctx.clone(), guild.id).await;
        }
    });

    StatusService::spawn(ctx.clone());

    let build_ctx = ctx.clone();
    tokio::spawn(async move {
        BuildService::init(build_ctx.clone()).await;
        BuildService::spawn(build_ctx);
    });

    let market_ctx = ctx.clone();
    tokio::spawn(async move {
        MarketService::init(market_ctx).await;
    });

    NotificationService::spawn(ctx);

    tracing::info!(
        user = %event.user.name,
        id = %event.user.id,
        "Logged in"
    );
}
