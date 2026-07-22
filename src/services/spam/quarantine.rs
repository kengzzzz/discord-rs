use deadpool_redis::Pool;
use mongodb::bson::{doc, to_bson};
use twilight_http::{Error as HttpError, api_error::ApiError, error::ErrorType};
use twilight_model::id::{
    Id,
    marker::{GuildMarker, RoleMarker, UserMarker},
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

use super::CACHE_TTL;

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
        let quarantine_role =
            RoleService::get_by_type(ctx, guild_id.get(), &RoleEnum::Quarantine).await;
        if let Some(role) = &quarantine_role
            && let Err(e) = ctx
                .http
                .remove_guild_member_role(guild_id, user_id, Id::new(role.role_id))
                .await
        {
            tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), error = %e, "failed to remove quarantine role");
        }
        let quarantine_role_id = quarantine_role.map(|role| role.role_id);
        // Owned copy: the cache reference is a DashMap shard guard and must not
        // be held across the awaits below.
        let cached_guild_roles: Option<std::collections::HashSet<u64>> = ctx
            .cache
            .guild_roles(guild_id)
            .map(|ids| ids.iter().map(|id| id.get()).collect());
        for id in record.roles.iter() {
            if Some(*id) == quarantine_role_id {
                tracing::warn!(
                    guild_id = guild_id.get(),
                    user_id = user_id.get(),
                    role_id = *id,
                    "snapshot contained the quarantine role; skipping restore"
                );
                continue;
            }
            if let Some(guild_roles) = &cached_guild_roles
                && !guild_roles.contains(id)
            {
                tracing::warn!(
                    guild_id = guild_id.get(),
                    user_id = user_id.get(),
                    role_id = *id,
                    "role no longer exists in guild; skipping restore"
                );
                continue;
            }
            let managed = ctx
                .cache
                .role(Id::new(*id))
                .map(|role| role.managed);
            if managed == Some(true) {
                tracing::warn!(
                    guild_id = guild_id.get(),
                    user_id = user_id.get(),
                    role_id = *id,
                    "managed role cannot be manually restored; skipping"
                );
                continue;
            }
            if let Err(e) = ctx
                .http
                .add_guild_member_role(guild_id, user_id, Id::new(*id))
                .await
            {
                if is_permanent_restore_error(&e) {
                    tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = *id, error = %e, "role permanently unrestorable (unknown role); skipping");
                    continue;
                }
                if is_permission_error(&e) {
                    tracing::error!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = *id, error = %e, "missing permissions to restore role; check the bot's role is above this role in the hierarchy; user remains quarantined until fixed");
                } else {
                    tracing::warn!(guild_id = guild_id.get(), user_id = user_id.get(), role_id = *id, error = %e, "failed to restore member role");
                }
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
                "token": token,
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
    let quarantine_role_id = RoleService::get_by_type(ctx, guild_id.get(), &RoleEnum::Quarantine)
        .await
        .map(|role| role.role_id);

    // `roles` is the snapshot to strip and restore later; `target` is the role
    // list submitted to Discord. Roles verify() could never restore stay out of
    // the snapshot: the quarantine role itself (re-quarantine), and managed
    // roles, which go in `target` instead so the swap never asks Discord to
    // remove what it refuses to remove. Roles absent from the cache are kept.
    let (roles, mut target): (Vec<Id<RoleMarker>>, Vec<Id<RoleMarker>>) = {
        let Some(member_ref) = ctx.cache.member(guild_id, user_id) else {
            tracing::warn!(
                guild_id = guild_id.get(),
                user_id = user_id.get(),
                "member missing from cache while materializing quarantine"
            );
            redis_delete(&ctx.redis, &key).await;
            return;
        };

        let mut roles = Vec::new();
        let mut target = Vec::new();
        for r in member_ref.roles().iter().copied() {
            if Some(r.get()) == quarantine_role_id {
                tracing::warn!(
                    guild_id = guild_id.get(),
                    user_id = user_id.get(),
                    role_id = r.get(),
                    "member already holds quarantine role; excluding from snapshot"
                );
                continue;
            }
            if ctx
                .cache
                .role(r)
                .map(|role| role.managed)
                == Some(true)
            {
                tracing::debug!(
                    guild_id = guild_id.get(),
                    user_id = user_id.get(),
                    role_id = r.get(),
                    "retaining managed role through quarantine; excluded from snapshot"
                );
                target.push(r);
                continue;
            }
            roles.push(r);
        }
        (roles, target)
    };
    if let Some(id) = quarantine_role_id {
        target.push(Id::new(id));
    }

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

    // All-or-nothing on Discord's side: on failure the member kept every role,
    // so drop the record and release the claim rather than leave a quarantine
    // that is_quarantined() would honour against a member who still has access.
    if let Err(e) = ctx
        .http
        .update_guild_member_roles(guild_id, user_id, &target)
        .await
    {
        tracing::error!(guild_id = record.guild_id, user_id = record.user_id, error = %e, "failed to swap member roles for quarantine; releasing claim");
        if let Err(e) = ctx
            .mongo
            .quarantines
            .delete_one(doc! {
                "guild_id": record.guild_id as i64,
                "user_id": record.user_id as i64,
                "token": token,
            })
            .await
        {
            tracing::error!(guild_id = record.guild_id, user_id = record.user_id, error = %e, "failed to delete quarantine record after role swap failure");
        }
        redis_delete(&ctx.redis, &key).await;
        return;
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

const UNKNOWN_ROLE_CODE: u64 = 10011;

/// Extract (HTTP status, Discord JSON error code) from an anyhow-wrapped
/// Discord API error, if it is one.
fn api_error_parts(error: &anyhow::Error) -> Option<(u16, Option<u64>)> {
    if let Some(ErrorType::Response { error, status, .. }) = error
        .downcast_ref::<HttpError>()
        .map(HttpError::kind)
    {
        let code = match error {
            ApiError::General(general) => Some(general.code),
            _ => None,
        };
        return Some((status.get(), code));
    }

    // Real twilight errors cannot be constructed in tests, so the mock client
    // injects its own typed error instead.
    #[cfg(any(test, feature = "test-utils"))]
    if let Some(mock) = error.downcast_ref::<crate::context::mock_http::MockHttpError>() {
        return Some((mock.status, mock.code));
    }

    None
}

fn is_permanent_restore_error(error: &anyhow::Error) -> bool {
    matches!(
        api_error_parts(error),
        Some((_, Some(UNKNOWN_ROLE_CODE)))
    )
}

fn is_permission_error(error: &anyhow::Error) -> bool {
    matches!(api_error_parts(error), Some((403, _)))
}

#[cfg(any(test, feature = "test-utils"))]
#[allow(dead_code)]
#[path = "tests/quarantine.rs"]
mod tests;
