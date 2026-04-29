use std::sync::Arc;

use async_trait::async_trait;
use twilight_model::gateway::payload::incoming::{GuildCreate, MessageDelete, MessageDeleteBulk};

use crate::{
    context::Context,
    events::{guild_create, message_delete},
    features::registry::FeatureSlice,
};

pub struct RoleMessagesFeature;

#[async_trait]
impl FeatureSlice for RoleMessagesFeature {
    async fn handle_guild_create(&self, ctx: Arc<Context>, event: GuildCreate) -> bool {
        guild_create::handle(ctx, event).await;
        true
    }

    async fn handle_message_delete(&self, ctx: Arc<Context>, event: MessageDelete) -> bool {
        message_delete::handle_single(ctx, event).await;
        true
    }

    async fn handle_message_delete_bulk(
        &self,
        ctx: Arc<Context>,
        event: MessageDeleteBulk,
    ) -> bool {
        message_delete::handle_bulk(ctx, event).await;
        true
    }
}
