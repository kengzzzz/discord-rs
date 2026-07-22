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

        Self::purge_cache(&ctx.redis, guild_id).await;

        Ok(())
    }

    pub async fn purge_cache(pool: &Pool, guild_id: u64) {
        redis_delete(pool, &cache_key(guild_id)).await;
    }
}

fn cache_key(guild_id: u64) -> String {
    format!("{CACHE_PREFIX}:guild-settings:{guild_id}")
}

#[cfg(test)]
mod tests {
    use crate::context::ContextBuilder;

    use super::*;

    #[tokio::test]
    async fn set_scam_detect_enabled_purges_stale_cache() {
        let ctx = ContextBuilder::new()
            .watchers(false)
            .build()
            .await
            .expect("failed to build context");
        let guild_id = 7_006_001;
        let key = cache_key(guild_id);
        ctx.redis_set_ex(
            &key,
            &GuildSettings { id: None, guild_id, scam_detect_enabled: true },
            CACHE_TTL,
        )
        .await;

        GuildSettingsService::set_scam_detect_enabled(&ctx, guild_id, false)
            .await
            .expect("failed to update guild setting");

        let cached: Option<GuildSettings> = ctx.redis_get(&key).await;
        assert!(
            cached.is_none(),
            "setter should purge stale cache instead of overwriting it"
        );

        assert!(!GuildSettingsService::scam_detect_enabled(&ctx, guild_id).await);
        let cached: GuildSettings = ctx
            .redis_get(&key)
            .await
            .expect("next read should repopulate settings cache");
        assert!(!cached.scam_detect_enabled);
    }

    #[tokio::test]
    async fn rapid_toggles_leave_cache_empty_until_next_read() {
        let ctx = ContextBuilder::new()
            .watchers(false)
            .build()
            .await
            .expect("failed to build context");
        let guild_id = 7_006_002;
        let key = cache_key(guild_id);

        GuildSettingsService::set_scam_detect_enabled(&ctx, guild_id, true)
            .await
            .expect("failed to enable scam detection");
        GuildSettingsService::set_scam_detect_enabled(&ctx, guild_id, false)
            .await
            .expect("failed to disable scam detection");

        let cached: Option<GuildSettings> = ctx.redis_get(&key).await;
        assert!(
            cached.is_none(),
            "rapid writes should not leave a stale write-through value"
        );

        assert!(!GuildSettingsService::scam_detect_enabled(&ctx, guild_id).await);
        let cached: GuildSettings = ctx
            .redis_get(&key)
            .await
            .expect("read-through should repopulate settings cache");
        assert_eq!(cached.guild_id, guild_id);
        assert!(!cached.scam_detect_enabled);
    }
}
