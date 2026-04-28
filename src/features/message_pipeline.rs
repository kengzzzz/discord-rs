use std::sync::Arc;

use async_trait::async_trait;
use twilight_model::channel::Message;

use crate::{context::Context, events::message_create, features::registry::FeatureSlice};

pub struct MessagePipelineFeature;

#[async_trait]
impl FeatureSlice for MessagePipelineFeature {
    async fn handle_message_create(&self, ctx: Arc<Context>, message: Message) -> bool {
        message_create::handle(ctx, message).await;
        true
    }
}
