use twilight_model::gateway::payload::incoming::MemberRemove;

use crate::{context::Context, features::member_onboarding};
use std::sync::Arc;

pub async fn handle(ctx: Arc<Context>, event: MemberRemove) {
    member_onboarding::handle_member_remove(ctx, event).await;
}
