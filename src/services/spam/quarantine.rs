use deadpool_redis::Pool;
use mongodb::bson::{doc, to_bson};
use twilight_model::id::{
    Id,
    marker::{GuildMarker, UserMarker},
};

use crate::{
    context::Context,
    dbs::{
        mongo::models::{quarantine::Quarantine, role::RoleEnum},
        redis::{redis_delete, redis_get, redis_set},
    },
    services::role::RoleService,
};
use std::sync::Arc;

pub async fn verify(
    ctx: &Arc<Context>,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    token: &str,
) -> bool {
    let key = format!(
        "spam:quarantine:{}:{}",
        guild_id.get(),
        user_id.get()
    );

    if let Some(stored) = redis_get::<String>(&ctx.redis, &key).await {
        if stored != token {
            return false;
        }
    }

    if let Ok(Some(record)) = ctx
        .mongo
        .quarantines
        .find_one(doc! {
            "guild_id": guild_id.get() as i64,
            "user_id": user_id.get() as i64,
            "token": token,
        })
        .await
    {
        if let Some(role) =
            RoleService::get_by_type(ctx, guild_id.get(), &RoleEnum::Quarantine).await
        {
            if let Err(e) = ctx
                .http
                .remove_guild_member_role(guild_id, user_id, Id::new(role.role_id))
                .await
            {
                tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), error = %e, "failed to remove quarantine role");
            }
        }
        for id in record.roles.iter() {
            if let Err(e) = ctx
                .http
                .add_guild_member_role(guild_id, user_id, Id::new(*id))
                .await
            {
                tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = *id, error = %e, "failed to restore member role");
            }
        }

        if let Err(e) = ctx
            .mongo
            .quarantines
            .delete_one(doc! {
                "guild_id": guild_id.get() as i64,
                "user_id": user_id.get() as i64,
            })
            .await
        {
            tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), error = %e, "failed to delete quarantine record");
        }

        return true;
    }

    false
}

pub async fn get_token(ctx: &Arc<Context>, guild_id: u64, user_id: u64) -> Option<String> {
    let key = format!("spam:quarantine:{guild_id}:{user_id}");
    if let Some(token) = redis_get::<String>(&ctx.redis, &key).await {
        return Some(token);
    }

    let token = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": guild_id as i64, "user_id": user_id as i64})
        .await
        .ok()
        .flatten()
        .map(|r| r.token);

    redis_set(&ctx.redis, &key, &token).await;

    token
}

pub async fn quarantine_member(
    ctx: &Arc<Context>,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    token: &str,
) {
    if let Some(member_ref) = ctx.cache.member(guild_id, user_id) {
        let roles = member_ref.roles();
        for r in roles {
            if let Err(e) = ctx
                .http
                .remove_guild_member_role(guild_id, user_id, *r)
                .await
            {
                tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = r.get(), error = %e, "failed to remove member role for quarantine");
            }
        }
        if let Some(role) =
            RoleService::get_by_type(ctx, guild_id.get(), &RoleEnum::Quarantine).await
        {
            if let Err(e) = ctx
                .http
                .add_guild_member_role(guild_id, user_id, Id::new(role.role_id))
                .await
            {
                tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = role.role_id, error = %e, "failed to assign quarantine role");
            }
        }
        let record = Quarantine {
            id: None,
            guild_id: guild_id.get(),
            user_id: user_id.get(),
            token: token.to_string(),
            roles: roles.iter().map(|r| r.get()).collect(),
        };
        if let Ok(bson) = to_bson(&record) {
            if let Err(e) = ctx
                .mongo
                .quarantines
                .update_one(
                    doc! {"guild_id": record.guild_id as i64, "user_id": record.user_id as i64},
                    doc! {"$set": bson},
                )
                .upsert(true)
                .await
            {
                tracing::warn!(guild_id = record.guild_id, user_id = record.user_id, error = %e, "failed to upsert quarantine record");
            }
        }
    }
}

pub async fn purge_cache(pool: &Pool, guild_id: u64, user_id: u64) {
    let log_key = format!("spam:log:{guild_id}:{user_id}");
    let quarantine_key = format!("spam:quarantine:{guild_id}:{user_id}");
    redis_delete(pool, &log_key).await;
    redis_delete(pool, &quarantine_key).await;
}

#[cfg(any(test, feature = "test-utils"))]
#[allow(dead_code)]
#[path = "tests/quarantine.rs"]
mod tests;
