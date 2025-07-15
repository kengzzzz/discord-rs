use deadpool_redis::Pool;
use mongodb::bson::doc;

use crate::{
    configs::CACHE_PREFIX,
    context::Context,
    dbs::{
        mongo::models::role::{Role, RoleEnum},
        redis::{redis_delete, redis_get, redis_set},
    },
};
use std::sync::Arc;

pub struct RoleService;

impl RoleService {
    pub async fn get(ctx: Arc<Context>, role_id: u64) -> Option<Role> {
        let redis_key = format!("{CACHE_PREFIX}:role:{role_id}");

        if let Some(role) = redis_get(&ctx.redis, &redis_key).await {
            return Some(role);
        }

        if let Ok(Some(role)) = ctx
            .mongo
            .roles
            .find_one(doc! {
                "role_id": role_id as i64
            })
            .await
        {
            redis_set(&ctx.redis, &redis_key, &role).await;
            return Some(role);
        }

        None
    }

    pub async fn purge_cache(pool: &Pool, role_id: u64) {
        let redis_key = format!("{CACHE_PREFIX}:role:{role_id}");
        redis_delete(pool, &redis_key).await;
    }

    pub async fn get_by_type(
        ctx: Arc<Context>,
        guild_id: u64,
        role_type: &RoleEnum,
    ) -> Option<Role> {
        let redis_key = format!(
            "{}:role-type:{}:{}",
            CACHE_PREFIX,
            guild_id,
            role_type.value()
        );

        if let Some(role) = redis_get(&ctx.redis, &redis_key).await {
            return Some(role);
        }

        if let Ok(Some(role)) = ctx
            .mongo
            .roles
            .find_one(doc! {"guild_id": guild_id as i64, "role_type": role_type.value()})
            .await
        {
            redis_set(&ctx.redis, &redis_key, &role).await;
            return Some(role);
        }

        None
    }

    pub async fn purge_cache_by_type(pool: &Pool, guild_id: u64, role_type: &RoleEnum) {
        let redis_key = format!(
            "{}:role-type:{}:{}",
            CACHE_PREFIX,
            guild_id,
            role_type.value()
        );
        redis_delete(pool, &redis_key).await;
    }
}
