use std::sync::Arc;

use async_trait::async_trait;
use twilight_model::gateway::payload::incoming::VoiceStateUpdate;

use crate::{context::Context, events::voice_state_update, features::registry::FeatureSlice};

pub struct VoiceLogsFeature;

#[async_trait]
impl FeatureSlice for VoiceLogsFeature {
    async fn handle_voice_state_update(&self, ctx: Arc<Context>, event: VoiceStateUpdate) -> bool {
        voice_state_update::handle(ctx, event).await;
        true
    }
}
