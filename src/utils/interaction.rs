use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::application::interaction::Interaction;
use twilight_model::id::{Id, marker::GuildMarker};

use crate::{context::Context, utils::embed};

pub async fn respond_cache_unavailable(ctx: &Context, interaction: &Interaction) {
    let embed = match embed::guild_unavailable_embed() {
        Ok(embed) => embed,
        Err(e) => {
            tracing::warn!(error = %e, "failed to build guild-unavailable embed");
            return;
        }
    };
    if let Err(e) = ctx
        .http
        .interaction(interaction.application_id)
        .update_response(&interaction.token)
        .embeds(Some(&[embed]))
        .await
    {
        tracing::warn!(error = %e, "failed to send guild-unavailable response");
    }
}

/// Resolves a guild from the cache, answering the already-deferred interaction
/// instead of leaving the user with "the application did not respond" when the
/// cache has not been populated yet.
pub async fn require_guild_ref<'a>(
    ctx: &'a Context,
    interaction: &Interaction,
    guild_id: Id<GuildMarker>,
    command: &str,
) -> Option<Reference<'a, Id<GuildMarker>, CachedGuild>> {
    match ctx.cache.guild(guild_id) {
        Some(guild_ref) => Some(guild_ref),
        None => {
            tracing::warn!(
                guild_id = guild_id.get(),
                command,
                "guild cache miss; responding with error"
            );
            respond_cache_unavailable(ctx, interaction).await;
            None
        }
    }
}
