pub mod cache;
pub mod client;
pub mod embed;

use reqwest::Client;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::message::Embed,
    id::{Id, marker::GuildMarker},
};

pub struct BuildService;

impl BuildService {
    pub(crate) fn sanitize_item_name(s: &str) -> String {
        let extra = s.bytes().filter(|&b| b == b'&').count() * 2;
        let mut out = String::with_capacity(s.len() + extra);
        for &b in s.as_bytes() {
            match b {
                b'A'..=b'Z' => out.push((b | 0x20) as char),
                b' ' => out.push('-'),
                b'&' => out.push_str("%26"),
                _ => out.push(b as char),
            }
        }
        out
    }

    pub async fn build_embeds(
        client: &Client,
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
        item: &str,
    ) -> anyhow::Result<Vec<Embed>> {
        let target = Self::sanitize_item_name(item);
        match client::fetch_builds(client, &target).await {
            Ok(builds) => Self::build_embeds_internal(guild, item, builds),
            Err(e) => {
                tracing::warn!(error = %e, "failed to fetch builds");
                Ok(vec![Self::build_error_embed(guild)?])
            }
        }
    }
}
