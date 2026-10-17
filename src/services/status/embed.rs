use std::sync::Arc;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::message::Embed,
    id::{Id, marker::GuildMarker},
};
use twilight_util::builder::embed::EmbedBuilder;

use crate::{
    context::Context,
    warframe::{self, api::CircuitData, circuit},
};

pub struct StatusSnapshot {
    cycles: Option<warframe::embed::StatusSnapshot>,
    circuit: Option<CircuitData>,
}

pub async fn fetch_snapshot(ctx: &Arc<Context>) -> StatusSnapshot {
    let (cycles, circuit) = tokio::join!(
        warframe::status_snapshot(ctx),
        circuit::circuit_data(ctx)
    );
    if let Err(error) = &cycles {
        tracing::warn!(error = %error, "failed to fetch cycle status snapshot");
    }
    if let Err(error) = &circuit {
        tracing::warn!(error = %error, "failed to fetch Circuit status snapshot");
    }
    StatusSnapshot { cycles: cycles.ok(), circuit: circuit.ok() }
}

pub fn build_embeds(
    snapshot: &StatusSnapshot,
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
) -> Vec<Embed> {
    let cycles = snapshot
        .cycles
        .as_ref()
        .and_then(|cycles| match warframe::status_embed_from_snapshot(cycles, guild) {
            Ok((embed, is_umbra)) => {
                super::StatusService::set_umbra_forma(is_umbra);
                Some(embed)
            }
            Err(error) => {
                tracing::warn!(error = %error, "failed to build cycle status embed");
                None
            }
        })
        .unwrap_or_else(|| {
            EmbedBuilder::new()
                .title("[PC] Warframe Cycle Timers")
                .description("Cycle timers and Steel Path are temporarily unavailable. Retrying automatically.")
                .color(0xE74C3C)
                .build()
        });
    let circuit = snapshot
        .circuit
        .as_ref()
        .and_then(|data| match circuit::status_embed(data) {
            Ok(embed) => Some(embed),
            Err(error) => {
                tracing::warn!(error = %error, "failed to build Circuit status embed");
                None
            }
        })
        .unwrap_or_else(circuit::unavailable_embed);
    vec![cycles, circuit]
}
