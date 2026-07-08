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
        redis::{redis_delete, redis_get, redis_set_ex, redis_set_nx_ex},
    },
    services::{role::RoleService, spam::log},
};
use std::sync::Arc;

const CACHE_TTL: usize = 3600;

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

    if let Some(stored) = redis_get::<String>(&ctx.redis, &key).await
        && stored != token
    {
        return false;
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
        .map(|record| record.filter(|record| !record.released))
    {
        let mut unrestored_roles = Vec::new();
        if let Some(role) =
            RoleService::get_by_type(ctx, guild_id.get(), &RoleEnum::Quarantine).await
            && let Err(e) = ctx
                .http
                .remove_guild_member_role(guild_id, user_id, Id::new(role.role_id))
                .await
        {
            tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), error = %e, "failed to remove quarantine role");
        }
        for id in record.roles.iter() {
            if let Err(e) = ctx
                .http
                .add_guild_member_role(guild_id, user_id, Id::new(*id))
                .await
            {
                tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = *id, error = %e, "failed to restore member role");
                unrestored_roles.push(*id);
            }
        }

        if !unrestored_roles.is_empty() {
            tracing::error!(
                guild_id = guild_id.get(),
                user_id = user_id.get(),
                unrestored_roles = ?unrestored_roles,
                saved_roles = ?record.roles,
                "failed to restore all quarantined member roles; keeping quarantine record for retry"
            );
            return false;
        }

        if let Err(e) = ctx
            .mongo
            .quarantines
            .update_one(
                doc! {
                    "guild_id": guild_id.get() as i64,
                    "user_id": user_id.get() as i64,
                    "token": token,
                },
                doc! {"$set": {"released": true}},
            )
            .upsert(false)
            .await
        {
            tracing::error!(guild_id = guild_id.get(), user_id = user_id.get(), error = %e, "failed to mark quarantine record released");
            return false;
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
            tracing::error!(guild_id = guild_id.get(), user_id = user_id.get(), error = %e, "failed to delete released quarantine record");
        }

        redis_delete(&ctx.redis, &key).await;
        log::clear_log(&ctx.redis, guild_id.get(), user_id.get()).await;

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
        .filter(|record| !record.released)
        .map(|r| r.token);

    if let Some(stored) = &token {
        redis_set_ex(&ctx.redis, &key, stored, CACHE_TTL).await;
    }

    token
}

pub async fn claim_token(
    ctx: &Arc<Context>,
    guild_id: u64,
    user_id: u64,
    token: &str,
) -> Result<String, Option<String>> {
    // A quarantine record may already exist in Mongo even when the Redis key is
    // missing (eviction/restart/flush). Check Mongo first so we reuse the token
    // that was already handed out instead of minting a new one Redis would
    // happily NX-claim, which would desync Redis from Mongo's token.
    if let Some(existing) = get_token(ctx, guild_id, user_id).await {
        return Err(Some(existing));
    }

    let key = quarantine_key(guild_id, user_id);
    if redis_set_nx_ex(&ctx.redis, &key, &token, CACHE_TTL).await {
        return Ok(token.to_owned());
    }

    Err(get_token(ctx, guild_id, user_id).await)
}

pub async fn quarantine_member(
    ctx: &Arc<Context>,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    token: &str,
) {
    let key = quarantine_key(guild_id.get(), user_id.get());
    let Some(member_ref) = ctx.cache.member(guild_id, user_id) else {
        tracing::warn!(
            guild_id = guild_id.get(),
            user_id = user_id.get(),
            "member missing from cache while materializing quarantine"
        );
        redis_delete(&ctx.redis, &key).await;
        return;
    };

    let roles = member_ref.roles().to_vec();
    let record = Quarantine {
        id: None,
        guild_id: guild_id.get(),
        user_id: user_id.get(),
        token: token.to_string(),
        roles: roles.iter().map(|r| r.get()).collect(),
        released: false,
    };

    let Ok(bson) = to_bson(&record) else {
        tracing::warn!(
            guild_id = record.guild_id,
            user_id = record.user_id,
            "failed to serialize quarantine record"
        );
        redis_delete(&ctx.redis, &key).await;
        return;
    };

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
        redis_delete(&ctx.redis, &key).await;
        return;
    }

    for r in &roles {
        if let Err(e) = ctx
            .http
            .remove_guild_member_role(guild_id, user_id, *r)
            .await
        {
            tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = r.get(), error = %e, "failed to remove member role for quarantine");
        }
    }
    if let Some(role) = RoleService::get_by_type(ctx, guild_id.get(), &RoleEnum::Quarantine).await
        && let Err(e) = ctx
            .http
            .add_guild_member_role(guild_id, user_id, Id::new(role.role_id))
            .await
    {
        tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = role.role_id, error = %e, "failed to assign quarantine role");
    }

    redis_set_ex(&ctx.redis, &key, &token, CACHE_TTL).await;
}

pub async fn purge_cache(pool: &Pool, guild_id: u64, user_id: u64) {
    let quarantine_key = quarantine_key(guild_id, user_id);
    log::clear_log(pool, guild_id, user_id).await;
    redis_delete(pool, &quarantine_key).await;
}

fn quarantine_key(guild_id: u64, user_id: u64) -> String {
    format!("spam:quarantine:{guild_id}:{user_id}")
}

#[cfg(any(test, feature = "test-utils"))]
#[allow(dead_code)]
#[path = "tests/quarantine.rs"]
mod tests;
