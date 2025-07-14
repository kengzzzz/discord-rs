use std::sync::{Arc, atomic::Ordering};
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::message::Embed,
    id::{Id, marker::GuildMarker},
};

use crate::{context::Context, warframe};

pub async fn build_embed(
    ctx: Arc<Context>,
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
) -> Option<Embed> {
    match warframe::status_embed(ctx.clone(), guild).await {
        Ok((e, is_umbra)) => {
            super::UMBRA_FORMA.store(is_umbra, Ordering::Relaxed);
            Some(e)
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to build status embed");
            None
        }
    }
}
