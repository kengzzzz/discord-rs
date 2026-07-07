use deadpool_redis::Pool;
use mongodb::bson::doc;

use crate::{
    configs::CACHE_PREFIX,
    context::Context,
    dbs::{
        mongo::models::guild_settings::GuildSettings,
        redis::{redis_delete, redis_get, redis_set_ex},
    },
};

const CACHE_TTL: usize = 3600;

pub struct GuildSettingsService;

impl GuildSettingsService {
    pub async fn get(ctx: &Context, guild_id: u64) -> GuildSettings {
        let redis_key = cache_key(guild_id);
        if let Some(settings) = redis_get(&ctx.redis, &redis_key).await {
            return settings;
        }

        let settings = ctx
            .mongo
            .guild_settings
            .find_one(doc! {"guild_id": guild_id as i64})
            .await
            .ok()
            .flatten()
            .unwrap_or(GuildSettings { id: None, guild_id, scam_detect_enabled: false });

        redis_set_ex(&ctx.redis, &redis_key, &settings, CACHE_TTL).await;
        settings
    }

    pub async fn scam_detect_enabled(ctx: &Context, guild_id: u64) -> bool {
        Self::get(ctx, guild_id)
            .await
            .scam_detect_enabled
    }

    pub async fn set_scam_detect_enabled(
        ctx: &Context,
        guild_id: u64,
        enabled: bool,
    ) -> anyhow::Result<()> {
        ctx.mongo
            .guild_settings
            .update_one(
                doc! {"guild_id": guild_id as i64},
                doc! {
                    "$set": {
                        "guild_id": guild_id as i64,
                        "scam_detect_enabled": enabled,
                    }
                },
            )
            .upsert(true)
            .await?;

        redis_set_ex(
            &ctx.redis,
            &cache_key(guild_id),
            &GuildSettings { id: None, guild_id, scam_detect_enabled: enabled },
            CACHE_TTL,
        )
        .await;

        Ok(())
    }

    pub async fn purge_cache(pool: &Pool, guild_id: u64) {
        redis_delete(pool, &cache_key(guild_id)).await;
    }
}

fn cache_key(guild_id: u64) -> String {
    format!("{CACHE_PREFIX}:guild-settings:{guild_id}")
}
