use std::sync::Arc;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::message::Embed,
    id::{Id, marker::GuildMarker},
};

use crate::{context::Context, warframe, warframe::embed::StatusSnapshot};

pub async fn fetch_snapshot(ctx: &Arc<Context>) -> Option<StatusSnapshot> {
    match warframe::status_snapshot(ctx).await {
        Ok(snapshot) => Some(snapshot),
        Err(e) => {
            tracing::warn!(error = %e, "failed to fetch status snapshot");
            None
        }
    }
}

pub fn build_embed(
    snapshot: &StatusSnapshot,
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
) -> Option<Embed> {
    match warframe::status_embed_from_snapshot(snapshot, guild) {
        Ok((e, is_umbra)) => {
            super::StatusService::set_umbra_forma(is_umbra);
            Some(e)
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to build status embed");
            None
        }
    }
}
