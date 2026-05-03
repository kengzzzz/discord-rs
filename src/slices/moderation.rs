use std::sync::Arc;

use async_trait::async_trait;
use twilight_model::channel::Message;

use crate::{context::Context, events::message_create, slices::registry::FeatureSlice};

pub struct ModerationSlice;

#[async_trait]
impl FeatureSlice for ModerationSlice {
    async fn handle_message_create(&self, ctx: Arc<Context>, message: Message) -> bool {
        message_create::handle(ctx, message).await;
        true
    }
}
