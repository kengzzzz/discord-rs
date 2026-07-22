use super::*;
use crate::context::{ContextBuilder, mock_http::MockClient as Client};
use crate::dbs::mongo::models::role::Role;

async fn build_context() -> Arc<Context> {
    let ctx = ContextBuilder::new()
        .http(Client::new())
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
}

async fn settle() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}

async fn seed_guild(ctx: &Arc<Context>, guild_id: u64, channel_id: u64, role_id: u64) {
    ctx.mongo
        .channels
        .insert_one(Channel {
            id: None,
            channel_type: ChannelEnum::Notification,
            channel_id,
            guild_id,
        })
        .await
        .unwrap();
    ctx.mongo
        .roles
        .insert_one(Role {
            id: None,
            role_type: RoleEnum::Helminth,
            role_id,
            guild_id,
            self_assignable: false,
        })
        .await
        .unwrap();
}

async fn clear_handles(guild_id: u64) {
    let mut guard = HANDLES.write().await;
    if let Some(old) = guard.remove(&guild_id) {
        for h in old {
            h.abort();
        }
    }
}

#[tokio::test]
async fn test_reload_guild_concurrent_no_task_leak() {
    shutdown::set_token(CancellationToken::new());
    let ctx = build_context().await;
    let guild_id = 9_100_001u64;
    seed_guild(&ctx, guild_id, 1, 2).await;
    clear_handles(guild_id).await;

    let before = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();

    // Two overlapping reload_guild calls for the same guild, e.g. an admin
    // updating notification config twice in quick succession.
    tokio::join!(
        NotificationService::reload_guild(&ctx, guild_id),
        NotificationService::reload_guild(&ctx, guild_id),
    );

    settle().await;

    let tracked_len = {
        let guard = HANDLES.read().await;
        guard
            .get(&guild_id)
            .map(|v| v.len())
            .unwrap_or(0)
    };
    assert_eq!(
        tracked_len, 1,
        "exactly one set of tasks should remain tracked in HANDLES"
    );

    let after = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();
    assert_eq!(
        after.saturating_sub(before),
        1,
        "the losing call's tasks must have been aborted, not left running untracked"
    );

    clear_handles(guild_id).await;
}

#[tokio::test]
async fn test_reload_guild_sequential_replaces_tasks() {
    shutdown::set_token(CancellationToken::new());
    let ctx = build_context().await;
    let guild_id = 9_100_002u64;
    seed_guild(&ctx, guild_id, 3, 4).await;
    clear_handles(guild_id).await;

    let before = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();

    NotificationService::reload_guild(&ctx, guild_id).await;
    settle().await;
    let after_first = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();
    assert_eq!(after_first.saturating_sub(before), 1);

    NotificationService::reload_guild(&ctx, guild_id).await;
    settle().await;
    let after_second = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();
    assert_eq!(
        after_second, after_first,
        "reloading again should replace, not accumulate, tracked tasks"
    );

    let tracked_len = {
        let guard = HANDLES.read().await;
        guard
            .get(&guild_id)
            .map(|v| v.len())
            .unwrap_or(0)
    };
    assert_eq!(tracked_len, 1);

    clear_handles(guild_id).await;
}

#[tokio::test]
async fn test_startup_merge_does_not_clobber_concurrent_reload() {
    shutdown::set_token(CancellationToken::new());
    let ctx = build_context().await;
    let guild_id = 9_100_003u64;
    seed_guild(&ctx, guild_id, 5, 6).await;
    clear_handles(guild_id).await;

    let before = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();

    let startup_map = NotificationService::init_all(ctx.clone(), shutdown::get_token()).await;
    NotificationService::reload_guild(&ctx, guild_id).await;
    NotificationService::merge_initial_handles(startup_map).await;
    settle().await;

    let tracked_len = {
        let guard = HANDLES.read().await;
        guard
            .get(&guild_id)
            .map(|v| v.len())
            .unwrap_or(0)
    };
    assert_eq!(
        tracked_len, 1,
        "a reload racing startup should remain the only tracked task set"
    );

    let after = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();
    assert_eq!(
        after.saturating_sub(before),
        1,
        "startup-spawned duplicates must be aborted instead of leaking untracked"
    );

    clear_handles(guild_id).await;
}
