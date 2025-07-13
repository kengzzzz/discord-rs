use twilight_model::gateway::payload::incoming::MemberRemove;

use crate::context::Context;
use crate::services::spam::SpamService;
use std::sync::Arc;

pub async fn handle(_ctx: Arc<Context>, event: MemberRemove) {
    if event.user.bot || event.user.system.unwrap_or_default() {
        return;
    }
    SpamService::clear_log(event.guild_id.get(), event.user.id.get()).await;
}
