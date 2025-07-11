use twilight_model::gateway::payload::incoming::Ready;

use crate::{
    dbs::mongo::mongodb::MongoDB,
    services::{
        build::BuildService, health::HealthService, market::MarketService,
        notification::NotificationService, role_message::RoleMessageService, status::StatusService,
    },
};

pub async fn handle(event: Ready) {
    MongoDB::init().await.expect("MongoDB initialize failed.");

    for guild in event.guilds {
        RoleMessageService::ensure_message(guild.id).await;
    }
    StatusService::spawn();
    BuildService::init().await;
    BuildService::spawn();
    MarketService::init().await;
    NotificationService::spawn();

    HealthService::set_ready(true);
    HealthService::set_discord(true);

    println!("Logged in.");
}
