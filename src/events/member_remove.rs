use twilight_model::gateway::payload::incoming::MemberRemove;

use crate::context::Context;
use crate::services::spam;
use std::sync::Arc;

pub async fn handle(ctx: Arc<Context>, event: MemberRemove) {
    if event.user.bot || event.user.system.unwrap_or_default() {
        return;
    }
    spam::log::clear_log(
        &ctx.redis,
        event.guild_id.get(),
        event.user.id.get(),
    )
    .await;
}
