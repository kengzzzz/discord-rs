use std::sync::Arc;

use chrono::{Duration, Utc};
use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};

use super::api::{self, CircuitData};
use crate::{
    configs::CACHE_PREFIX,
    context::Context,
    dbs::redis::{redis_get, redis_set_ex},
};

const CACHE_TTL: i64 = 5 * 60;
const COLOR: u32 = 0xF1C40F;
const STATUS_UPCOMING: usize = 3;
const SCHEDULE_NOTE: &str = "Upcoming weeks follow DE's Update 43 order";

// DE's nine-week offering order, updated in Update 43:
// https://www.warframe.com/en/patch-notes/pc/43-0-0
const ROTATIONS: [[&str; 5]; 9] = [
    ["Braton", "Lato", "Skana", "Paris", "Kunai"],
    ["Boar", "Gammacor", "Angstrum", "Gorgon", "Anku"],
    ["Bo", "Latron", "Furis", "Furax", "Strun"],
    ["Lex", "Magistar", "Boltor", "Bronco", "Ceramic Dagger"],
    ["Torid", "Dual Toxocyst", "Dual Ichor", "Miter", "Atomos"],
    ["Ack & Brunt", "Soma", "Vasto", "Nami Solo", "Burston"],
    ["Zylok", "Sibear", "Dread", "Despair", "Hate"],
    ["Dera", "Sybaris", "Cestra", "Sicarus", "Okina"],
    ["Vectis", "Stug", "Ballistica", "Destreza", "Obex"],
];

pub async fn circuit_data(ctx: &Arc<Context>) -> anyhow::Result<CircuitData> {
    let now = Utc::now();
    let key = format!("{CACHE_PREFIX}:wf:circuit");
    if let Some(data) = redis_get::<CircuitData>(&ctx.redis, &key).await
        && data.activation <= now
        && now < data.expiry
    {
        return Ok(data);
    }
    let data = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        api::circuit(&ctx.reqwest, now),
    )
    .await??;
    let ttl = (data.expiry - now)
        .num_seconds()
        .clamp(1, CACHE_TTL) as usize;
    redis_set_ex(&ctx.redis, &key, &data, ttl).await;
    Ok(data)
}

pub fn status_embed(data: &CircuitData) -> anyhow::Result<Embed> {
    build_embed(
        data,
        STATUS_UPCOMING,
        &format!("{SCHEDULE_NOTE} • Full schedule: /warframe circuit"),
    )
}

pub fn schedule_embed(data: &CircuitData) -> anyhow::Result<Embed> {
    build_embed(data, ROTATIONS.len() - 1, SCHEDULE_NOTE)
}

pub fn unavailable_embed() -> Embed {
    EmbedBuilder::new()
        .title("Circuit unavailable")
        .description("The current Circuit rotation is temporarily unavailable.")
        .color(0xE74C3C)
        .build()
}

fn wiki_links(names: &[String]) -> String {
    names
        .iter()
        .map(|name| {
            format!(
                "[{name}](https://wiki.warframe.com/w/{})",
                name.replace(' ', "_")
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn rotation_index(weapons: &[String]) -> Option<usize> {
    // Match the live set instead of assuming an epoch that game updates can move.
    ROTATIONS.iter().position(|rotation| {
        weapons.len() == rotation.len()
            && rotation.iter().all(|weapon| {
                weapons
                    .iter()
                    .any(|live| live == weapon)
            })
    })
}

fn build_embed(data: &CircuitData, upcoming: usize, footer: &str) -> anyhow::Result<Embed> {
    let mut description = format!(
        "**Incarnons (Steel Path)**\n{}",
        wiki_links(&data.weapons)
    );
    if !data.warframes.is_empty() {
        description.push_str(&format!(
            "\n**Warframes (Normal)**\n{}",
            wiki_links(&data.warframes)
        ));
    }
    match rotation_index(&data.weapons) {
        Some(index) => {
            description.push_str("\n\n**Next Incarnons**");
            for offset in 1..=upcoming.min(ROTATIONS.len() - 1) {
                let starts = (data.expiry + Duration::weeks(offset as i64 - 1)).timestamp();
                let when =
                    if offset == 1 { format!("<t:{starts}:R>") } else { format!("<t:{starts}:d>") };
                description.push_str(&format!(
                    "\n{when} · {}",
                    ROTATIONS[(index + offset) % ROTATIONS.len()].join(", ")
                ));
            }
        }
        None => description.push_str(&format!(
            "\n\n**Resets** <t:{}:R>\nUpcoming Incarnons are unknown for this set.",
            data.expiry.timestamp()
        )),
    }

    Ok(EmbedBuilder::new()
        .title("Circuit Rotation")
        .url("https://wiki.warframe.com/w/The_Circuit")
        .color(COLOR)
        .description(description)
        .footer(EmbedFooterBuilder::new(footer))
        .validate()?
        .build())
}

#[cfg(test)]
#[path = "tests/circuit.rs"]
mod tests;
