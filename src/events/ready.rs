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
    for guild in event.guilds {
        RoleMessageService::ensure_message(ctx.clone(), guild.id).await;
    }
    StatusService::spawn(ctx.clone());
    BuildService::init(ctx.clone()).await;
    BuildService::spawn(ctx.clone());
    MarketService::init(ctx.clone()).await;
    NotificationService::spawn(ctx);

    HealthService::set_ready(true);
    HealthService::set_discord(true);

    println!("Logged in.");
}
