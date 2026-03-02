use std::sync::Arc;
use twilight_model::gateway::payload::incoming::GuildCreate;

use crate::{context::Context, services::role_message};

pub async fn handle(ctx: Arc<Context>, event: GuildCreate) {
    let guild_id = event.id();

    tokio::spawn(async move {
        role_message::handler::ensure_message(&ctx, guild_id).await;
    });
}
