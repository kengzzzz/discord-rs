use twilight_model::gateway::payload::incoming::MemberRemove;

use crate::services::spam::SpamService;

pub async fn handle(event: MemberRemove) {
    if event.user.bot | event.user.system.unwrap_or_default() {
        return;
    }
    SpamService::clear_log(event.guild_id.get(), event.user.id.get()).await;
}
