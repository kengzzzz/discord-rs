use twilight_model::gateway::payload::incoming::MemberAdd;

use crate::{context::Context, features::member_onboarding};
use std::sync::Arc;

pub async fn handle(ctx: Arc<Context>, event: MemberAdd) {
    member_onboarding::handle_member_add(ctx, event).await;
}
