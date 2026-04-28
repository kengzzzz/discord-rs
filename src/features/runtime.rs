use std::sync::Arc;

use async_trait::async_trait;
use twilight_model::gateway::payload::incoming::Ready;

use crate::{context::Context, events::ready, features::registry::FeatureSlice};

pub struct RuntimeFeature;

#[async_trait]
impl FeatureSlice for RuntimeFeature {
    async fn handle_ready(&self, ctx: Arc<Context>, event: Ready) -> bool {
        ready::handle(ctx, event).await;
        true
    }
}
