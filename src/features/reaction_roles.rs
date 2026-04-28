use std::sync::Arc;

use async_trait::async_trait;
use twilight_model::gateway::payload::incoming::{ReactionAdd, ReactionRemove};

use crate::{
    context::Context,
    events::{reaction_add, reaction_remove},
    features::registry::FeatureSlice,
};

pub struct ReactionRolesFeature;

#[async_trait]
impl FeatureSlice for ReactionRolesFeature {
    async fn handle_reaction_add(&self, ctx: Arc<Context>, event: ReactionAdd) -> bool {
        reaction_add::handle(ctx, event).await;
        true
    }

    async fn handle_reaction_remove(&self, ctx: Arc<Context>, event: ReactionRemove) -> bool {
        reaction_remove::handle(ctx, event).await;
        true
    }
}
