use super::*;
use crate::context::{ContextBuilder, mock_http::MockClient as Client};
use crate::dbs::mongo::models::role::Role as DbRole;
#[allow(unused_imports)]
use crate::dbs::redis::{redis_set, redis_set_ex, redis_ttl};
use mongodb::bson::doc;
use serde::{Deserialize, Serialize};
use twilight_cache_inmemory::DefaultInMemoryCache;
use twilight_model::{
    gateway::payload::incoming::GuildCreate,
    guild::{
        AfkTimeout, DefaultMessageNotificationLevel, ExplicitContentFilter, Guild, Member,
        MemberFlags, MfaLevel, NSFWLevel, Permissions, PremiumTier, Role as GuildRole, RoleColors,
        RoleFlags, SystemChannelFlags, VerificationLevel,
    },
    user::User,
};

#[derive(Serialize, Deserialize)]
struct TestCampaignRecord {
    histories: Vec<(u64, u64)>,
    first_seen: i64,
    last_seen: i64,
}

async fn build_context() -> Arc<Context> {
    let ctx = ContextBuilder::new()
        .http(Client::new())
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
}

async fn reset_quarantine_state(ctx: &Arc<Context>, guild_id: u64, user_id: u64) {
    purge_cache(&ctx.redis, guild_id, user_id).await;
    ctx.mongo
        .quarantines
        .delete_many(doc! {"guild_id": guild_id as i64, "user_id": user_id as i64})
        .await
        .expect("failed to clear quarantine records");
}

#[allow(deprecated)]
fn guild_role(id: u64, managed: bool) -> GuildRole {
    GuildRole {
        color: 0,
        colors: RoleColors { primary_color: 0, secondary_color: None, tertiary_color: None },
        hoist: false,
        icon: None,
        id: Id::new(id),
        managed,
        mentionable: false,
        name: format!("role-{id}"),
        permissions: Permissions::empty(),
        position: 1,
        flags: RoleFlags::empty(),
        tags: None,
        unicode_emoji: None,
    }
}

async fn seed_quarantine_role(ctx: &Arc<Context>, guild_id: u64, role_id: u64) {
    RoleService::purge_cache_by_type(&ctx.redis, guild_id, &RoleEnum::Quarantine).await;
    ctx.mongo
        .roles
        .insert_one(DbRole {
            id: None,
            role_type: RoleEnum::Quarantine,
            role_id,
            guild_id,
            self_assignable: false,
        })
        .await
        .expect("failed to seed quarantine role");
}

fn cache_member(cache: &DefaultInMemoryCache, guild_id: u64, user_id: u64, roles: Vec<u64>) {
    cache_member_with_guild_roles(cache, guild_id, user_id, roles, Vec::new());
}

fn cache_member_with_guild_roles(
    cache: &DefaultInMemoryCache,
    guild_id: u64,
    user_id: u64,
    roles: Vec<u64>,
    guild_roles: Vec<GuildRole>,
) {
    let member = Member {
        avatar: None,
        avatar_decoration_data: None,
        banner: None,
        communication_disabled_until: None,
        deaf: false,
        flags: MemberFlags::empty(),
        joined_at: None,
        mute: false,
        nick: None,
        pending: false,
        premium_since: None,
        roles: roles
            .into_iter()
            .map(Id::<RoleMarker>::new)
            .collect(),
        user: User {
            accent_color: None,
            avatar: None,
            avatar_decoration: None,
            avatar_decoration_data: None,
            banner: None,
            bot: false,
            discriminator: 0,
            email: None,
            flags: None,
            global_name: None,
            id: Id::new(user_id),
            locale: None,
            mfa_enabled: None,
            name: "tester".into(),
            premium_type: None,
            primary_guild: None,
            public_flags: None,
            system: None,
            verified: None,
        },
    };

    let guild = Guild {
        afk_channel_id: None,
        afk_timeout: AfkTimeout::FIVE_MINUTES,
        application_id: None,
        approximate_member_count: None,
        approximate_presence_count: None,
        banner: None,
        channels: Vec::new(),
        default_message_notifications: DefaultMessageNotificationLevel::Mentions,
        description: None,
        discovery_splash: None,
        emojis: Vec::new(),
        explicit_content_filter: ExplicitContentFilter::None,
        features: Vec::new(),
        guild_scheduled_events: Vec::new(),
        icon: None,
        id: Id::new(guild_id),
        joined_at: None,
        large: false,
        max_members: None,
        max_presences: None,
        max_stage_video_channel_users: None,
        max_video_channel_users: None,
        member_count: None,
        members: vec![member],
        mfa_level: MfaLevel::None,
        name: "guild".into(),
        nsfw_level: NSFWLevel::Default,
        owner_id: Id::new(1),
        owner: None,
        permissions: None,
        preferred_locale: "en_us".into(),
        premium_progress_bar_enabled: false,
        premium_subscription_count: None,
        premium_tier: PremiumTier::None,
        presences: Vec::new(),
        public_updates_channel_id: None,
        roles: guild_roles,
        rules_channel_id: None,
        safety_alerts_channel_id: None,
        splash: None,
        stage_instances: Vec::new(),
        stickers: Vec::new(),
        system_channel_flags: SystemChannelFlags::empty(),
        system_channel_id: None,
        threads: Vec::new(),
        unavailable: Some(false),
        vanity_url_code: None,
        verification_level: VerificationLevel::None,
        voice_states: Vec::new(),
        widget_channel_id: None,
        widget_enabled: None,
    };

    cache.update(&GuildCreate::Available(guild));
}

#[tokio::test]
async fn test_get_token_from_redis() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 1).await;
    let key = "spam:quarantine:1:1";
    redis_set_ex(
        &ctx.redis,
        key,
        &"redis_token",
        crate::services::spam::CACHE_TTL,
    )
    .await;
    let record = Quarantine {
        id: None,
        guild_id: 1,
        user_id: 1,
        token: "mongo_token".into(),
        roles: Vec::new(),
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    let token = get_token(&ctx, 1, 1).await;
    assert_eq!(token, Some("redis_token".into()));
}

#[tokio::test]
async fn test_get_token_fallback_to_mongo() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 2).await;
    let record = Quarantine {
        id: None,
        guild_id: 1,
        user_id: 2,
        token: "mongo_token".into(),
        roles: Vec::new(),
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    let token = get_token(&ctx, 1, 2).await;
    assert_eq!(token, Some("mongo_token".into()));
    let key = "spam:quarantine:1:2";
    let cached: String = redis_get(&ctx.redis, key)
        .await
        .unwrap();
    assert_eq!(cached, "mongo_token");
}

#[tokio::test]
async fn test_purge_cache() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 3).await;
    let log_key = "spam:log:1:3";
    let quarantine_key = "spam:quarantine:1:3";
    redis_set(&ctx.redis, log_key, &1).await;
    redis_set_ex(
        &ctx.redis,
        quarantine_key,
        &"tok",
        crate::services::spam::CACHE_TTL,
    )
    .await;
    purge_cache(&ctx.redis, 1, 3).await;
    let log: Option<i32> = redis_get(&ctx.redis, log_key).await;
    let quarantine: Option<String> = redis_get(&ctx.redis, quarantine_key).await;
    assert!(log.is_none());
    assert!(quarantine.is_none());
}

#[tokio::test]
async fn test_verify_success_and_delete_record() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 4).await;
    let record = Quarantine {
        id: None,
        guild_id: 1,
        user_id: 4,
        token: "token".into(),
        roles: Vec::new(),
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:1:4",
        &"token",
        crate::services::spam::CACHE_TTL,
    )
    .await;
    redis_set(&ctx.redis, "spam:log:1:4", &123).await;
    let ok = verify(&ctx, Id::new(1), Id::new(4), "token").await;
    assert!(ok);
    let remaining = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 1i64, "user_id": 4i64})
        .await
        .unwrap();
    assert!(remaining.is_none());
    let cached_quarantine: Option<String> = redis_get(&ctx.redis, "spam:quarantine:1:4").await;
    assert!(cached_quarantine.is_none());
    let cached_log: Option<i32> = redis_get(&ctx.redis, "spam:log:1:4").await;
    assert!(cached_log.is_none());
}

#[tokio::test]
async fn test_verify_delete_spares_record_from_concurrent_requarantine() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 21).await;
    // "new" first: an unscoped delete removes the first match, so this ordering
    // is what makes the test fail without the token filter.
    for (token, roles) in [("new", vec![77u64]), ("old", Vec::new())] {
        ctx.mongo
            .quarantines
            .insert_one(Quarantine {
                id: None,
                guild_id: 1,
                user_id: 21,
                token: token.into(),
                roles,
                released: false,
            })
            .await
            .unwrap();
    }
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:1:21",
        &"old",
        crate::services::spam::CACHE_TTL,
    )
    .await;

    let ok = verify(&ctx, Id::new(1), Id::new(21), "old").await;
    assert!(ok);

    let released = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 1i64, "user_id": 21i64, "token": "old"})
        .await
        .unwrap();
    assert!(
        released.is_none(),
        "verified record should be deleted"
    );

    let survivor = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 1i64, "user_id": 21i64, "token": "new"})
        .await
        .unwrap()
        .expect("re-quarantine record must survive the released record's delete");
    assert!(!survivor.released);
    assert_eq!(survivor.roles, vec![77]);
}

#[tokio::test]
async fn test_verify_fails_on_mismatched_token() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 5).await;
    let record = Quarantine {
        id: None,
        guild_id: 1,
        user_id: 5,
        token: "token".into(),
        roles: Vec::new(),
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:1:5",
        &"other",
        crate::services::spam::CACHE_TTL,
    )
    .await;
    let ok = verify(&ctx, Id::new(1), Id::new(5), "token").await;
    assert!(!ok);
    let remaining = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 1i64, "user_id": 5i64})
        .await
        .unwrap();
    assert!(remaining.is_some());
}

#[tokio::test]
async fn test_verify_delete_failure_marks_record_released() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 10).await;
    let record = Quarantine {
        id: None,
        guild_id: 1,
        user_id: 10,
        token: "token".into(),
        roles: Vec::new(),
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:1:10",
        &"token",
        crate::services::spam::CACHE_TTL,
    )
    .await;
    redis_set(&ctx.redis, "spam:log:1:10", &123).await;
    ctx.mongo
        .quarantines
        .fail_next_delete_one();

    let ok = verify(&ctx, Id::new(1), Id::new(10), "token").await;
    assert!(ok);

    let remaining = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 1i64, "user_id": 10i64})
        .await
        .unwrap()
        .expect("released tombstone should remain after failed delete");
    assert!(remaining.released);

    let cached_quarantine: Option<String> = redis_get(&ctx.redis, "spam:quarantine:1:10").await;
    assert!(cached_quarantine.is_none());
    let cached_log: Option<i32> = redis_get(&ctx.redis, "spam:log:1:10").await;
    assert!(cached_log.is_none());
    assert!(!crate::services::spam::SpamService::is_quarantined(&ctx, 1, 10).await);
    let resurrected: Option<String> = redis_get(&ctx.redis, "spam:quarantine:1:10").await;
    assert!(resurrected.is_none());
}

#[tokio::test]
async fn test_verify_role_restore_failure_keeps_active_record_for_retry() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 11).await;
    let record = Quarantine {
        id: None,
        guild_id: 1,
        user_id: 11,
        token: "token".into(),
        roles: vec![10, 11],
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:1:11",
        &"token",
        crate::services::spam::CACHE_TTL,
    )
    .await;
    ctx.http
        .fail_next_add_guild_member_role();

    let ok = verify(&ctx, Id::new(1), Id::new(11), "token").await;
    assert!(!ok);

    let remaining = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 1i64, "user_id": 11i64})
        .await
        .unwrap()
        .expect("active quarantine record should remain after partial restore");
    assert!(!remaining.released);
    assert_eq!(remaining.roles, vec![10, 11]);

    let cached_quarantine: Option<String> = redis_get(&ctx.redis, "spam:quarantine:1:11").await;
    assert_eq!(cached_quarantine, Some("token".into()));
}

#[tokio::test]
async fn test_claim_token_reuses_mongo_token_after_redis_loss() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 7).await;

    let record = Quarantine {
        id: None,
        guild_id: 1,
        user_id: 7,
        token: "token-a".into(),
        roles: Vec::new(),
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:1:7",
        &"token-a",
        crate::services::spam::CACHE_TTL,
    )
    .await;

    // Simulate Redis losing the key (restart/eviction/flush) while Mongo still
    // has the record with the token that was already DMed to the user.
    redis_delete(&ctx.redis, "spam:quarantine:1:7").await;

    let claimed = claim_token(&ctx, 1, 7, "token-b").await;
    assert_eq!(claimed, Err(Some("token-a".into())));

    // Redis should be repopulated with the original token, not overwritten.
    let cached: Option<String> = redis_get(&ctx.redis, "spam:quarantine:1:7").await;
    assert_eq!(cached, Some("token-a".into()));

    // The user's original verification link (token A) must still work.
    let ok = verify(&ctx, Id::new(1), Id::new(7), "token-a").await;
    assert!(ok);
}

#[tokio::test]
async fn test_is_quarantined_mongo_fallback_repopulates_cache_with_ttl() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 12).await;

    let record = Quarantine {
        id: None,
        guild_id: 1,
        user_id: 12,
        token: "token".into(),
        roles: Vec::new(),
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();

    let key = "spam:quarantine:1:12";
    assert_eq!(redis_ttl(key).await, None);

    assert!(crate::services::spam::SpamService::is_quarantined(&ctx, 1, 12).await);

    let cached: Option<String> = redis_get(&ctx.redis, key).await;
    assert_eq!(cached, Some("token".into()));
    assert_eq!(
        redis_ttl(key).await,
        Some(crate::services::spam::CACHE_TTL)
    );
}

#[tokio::test]
async fn test_quarantine_member_releases_claim_when_member_missing_from_cache() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 8).await;

    let claimed = claim_token(&ctx, 1, 8, "token").await;
    assert_eq!(claimed, Ok("token".into()));

    quarantine_member(&ctx, Id::new(1), Id::new(8), "token").await;

    let record = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 1i64, "user_id": 8i64})
        .await
        .unwrap();
    assert!(record.is_none());

    let cached: Option<String> = redis_get(&ctx.redis, "spam:quarantine:1:8").await;
    assert!(cached.is_none());
}

#[tokio::test]
async fn test_quarantine_member_releases_claim_when_mongo_upsert_fails() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 9).await;
    cache_member(&ctx.cache, 1, 9, vec![10, 11]);

    let claimed = claim_token(&ctx, 1, 9, "token").await;
    assert_eq!(claimed, Ok("token".into()));
    ctx.mongo
        .quarantines
        .fail_next_update_one();

    quarantine_member(&ctx, Id::new(1), Id::new(9), "token").await;

    let record = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 1i64, "user_id": 9i64})
        .await
        .unwrap();
    assert!(record.is_none());

    let cached: Option<String> = redis_get(&ctx.redis, "spam:quarantine:1:9").await;
    assert!(cached.is_none());
}

#[tokio::test]
async fn test_verify_clears_campaign_records() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 1, 6).await;
    let record = Quarantine {
        id: None,
        guild_id: 1,
        user_id: 6,
        token: "token".into(),
        roles: Vec::new(),
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();

    let campaign_key = "spam:campaign:1:6:abc";
    redis_set(
        &ctx.redis,
        campaign_key,
        &TestCampaignRecord { histories: vec![(1, 10)], first_seen: 1, last_seen: 1 },
    )
    .await;
    redis_set(
        &ctx.redis,
        "spam:campaign:1:6",
        &vec![campaign_key.to_owned()],
    )
    .await;
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:1:6",
        &"token",
        crate::services::spam::CACHE_TTL,
    )
    .await;

    let ok = verify(&ctx, Id::new(1), Id::new(6), "token").await;
    assert!(ok);

    let campaign: Option<TestCampaignRecord> = redis_get(&ctx.redis, campaign_key).await;
    let campaign_index: Option<Vec<String>> = redis_get(&ctx.redis, "spam:campaign:1:6").await;
    assert!(campaign.is_none());
    assert!(campaign_index.is_none());
}

#[tokio::test]
async fn test_quarantine_snapshot_excludes_managed_roles() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 101, 31).await;
    seed_quarantine_role(&ctx, 101, 900).await;
    cache_member_with_guild_roles(
        &ctx.cache,
        101,
        31,
        vec![10, 11, 12],
        vec![guild_role(10, false), guild_role(11, true), guild_role(12, false)],
    );
    ctx.http.set_member_roles(
        Id::new(101),
        Id::new(31),
        vec![Id::new(10), Id::new(11), Id::new(12)],
    );

    let claimed = claim_token(&ctx, 101, 31, "token").await;
    assert_eq!(claimed, Ok("token".into()));
    quarantine_member(&ctx, Id::new(101), Id::new(31), "token").await;

    let record = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 101i64, "user_id": 31i64})
        .await
        .unwrap()
        .expect("quarantine record should exist");
    assert_eq!(record.roles, vec![10, 12]);

    // Managed role 11 was never stripped; quarantine role 900 was granted.
    assert_eq!(
        ctx.http
            .member_roles(Id::new(101), Id::new(31)),
        vec![Id::new(11), Id::new(900)]
    );
}

#[tokio::test]
async fn test_requarantine_snapshot_excludes_quarantine_role() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 102, 32).await;
    seed_quarantine_role(&ctx, 102, 900).await;
    cache_member_with_guild_roles(
        &ctx.cache,
        102,
        32,
        vec![10, 900],
        vec![guild_role(10, false), guild_role(900, false)],
    );
    ctx.http.set_member_roles(
        Id::new(102),
        Id::new(32),
        vec![Id::new(10), Id::new(900)],
    );

    let claimed = claim_token(&ctx, 102, 32, "token").await;
    assert_eq!(claimed, Ok("token".into()));
    quarantine_member(&ctx, Id::new(102), Id::new(32), "token").await;

    let record = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 102i64, "user_id": 32i64})
        .await
        .unwrap()
        .expect("quarantine record should exist");
    assert_eq!(record.roles, vec![10]);

    // The quarantine role was neither snapshotted nor stripped-then-re-added.
    assert_eq!(
        ctx.http
            .member_roles(Id::new(102), Id::new(32)),
        vec![Id::new(900)]
    );
}

#[tokio::test]
async fn test_verify_skips_role_deleted_from_guild() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 103, 33).await;
    // Guild role set is cached and contains role 10 but not 99.
    cache_member_with_guild_roles(
        &ctx.cache,
        103,
        33,
        Vec::new(),
        vec![guild_role(10, false)],
    );
    let record = Quarantine {
        id: None,
        guild_id: 103,
        user_id: 33,
        token: "token".into(),
        roles: vec![10, 99],
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:103:33",
        &"token",
        crate::services::spam::CACHE_TTL,
    )
    .await;

    let ok = verify(&ctx, Id::new(103), Id::new(33), "token").await;
    assert!(ok);

    let remaining = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 103i64, "user_id": 33i64})
        .await
        .unwrap();
    assert!(remaining.is_none());
    assert_eq!(
        ctx.http
            .member_roles(Id::new(103), Id::new(33)),
        vec![Id::new(10)]
    );
    let cached: Option<String> = redis_get(&ctx.redis, "spam:quarantine:103:33").await;
    assert!(cached.is_none());
}

#[tokio::test]
async fn test_verify_skips_managed_role_in_legacy_snapshot() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 104, 34).await;
    cache_member_with_guild_roles(
        &ctx.cache,
        104,
        34,
        Vec::new(),
        vec![guild_role(10, false), guild_role(11, true)],
    );
    let record = Quarantine {
        id: None,
        guild_id: 104,
        user_id: 34,
        token: "token".into(),
        roles: vec![10, 11],
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:104:34",
        &"token",
        crate::services::spam::CACHE_TTL,
    )
    .await;

    let ok = verify(&ctx, Id::new(104), Id::new(34), "token").await;
    assert!(ok);

    let remaining = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 104i64, "user_id": 34i64})
        .await
        .unwrap();
    assert!(remaining.is_none());
    assert_eq!(
        ctx.http
            .member_roles(Id::new(104), Id::new(34)),
        vec![Id::new(10)]
    );
}

#[tokio::test]
async fn test_verify_skips_unknown_role_http_error() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 105, 35).await;
    // Cold cache for guild 105: pre-checks are bypassed, HTTP classification
    // must handle the 404 Unknown Role response.
    let record = Quarantine {
        id: None,
        guild_id: 105,
        user_id: 35,
        token: "token".into(),
        roles: vec![10, 99],
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:105:35",
        &"token",
        crate::services::spam::CACHE_TTL,
    )
    .await;
    ctx.http
        .fail_add_guild_member_role_with(Id::new(99), 404, Some(10011));

    let ok = verify(&ctx, Id::new(105), Id::new(35), "token").await;
    assert!(ok);

    let remaining = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 105i64, "user_id": 35i64})
        .await
        .unwrap();
    assert!(remaining.is_none());
    assert_eq!(
        ctx.http
            .member_roles(Id::new(105), Id::new(35)),
        vec![Id::new(10)]
    );
    let cached: Option<String> = redis_get(&ctx.redis, "spam:quarantine:105:35").await;
    assert!(cached.is_none());
}

#[tokio::test]
async fn test_verify_transient_500_keeps_record() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 106, 36).await;
    let record = Quarantine {
        id: None,
        guild_id: 106,
        user_id: 36,
        token: "token".into(),
        roles: vec![10],
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:106:36",
        &"token",
        crate::services::spam::CACHE_TTL,
    )
    .await;
    ctx.http
        .fail_add_guild_member_role_with(Id::new(10), 500, None);

    let ok = verify(&ctx, Id::new(106), Id::new(36), "token").await;
    assert!(!ok);

    let remaining = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 106i64, "user_id": 36i64})
        .await
        .unwrap()
        .expect("active quarantine record should remain after transient failure");
    assert!(!remaining.released);
    assert_eq!(remaining.roles, vec![10]);
    let cached: Option<String> = redis_get(&ctx.redis, "spam:quarantine:106:36").await;
    assert_eq!(cached, Some("token".into()));
}

#[tokio::test]
async fn test_verify_permission_403_keeps_record() {
    let ctx = build_context().await;
    reset_quarantine_state(&ctx, 107, 37).await;
    let record = Quarantine {
        id: None,
        guild_id: 107,
        user_id: 37,
        token: "token".into(),
        roles: vec![10],
        released: false,
    };
    ctx.mongo
        .quarantines
        .insert_one(record)
        .await
        .unwrap();
    redis_set_ex(
        &ctx.redis,
        "spam:quarantine:107:37",
        &"token",
        crate::services::spam::CACHE_TTL,
    )
    .await;
    ctx.http
        .fail_add_guild_member_role_with(Id::new(10), 403, Some(50013));

    let ok = verify(&ctx, Id::new(107), Id::new(37), "token").await;
    assert!(!ok);

    let remaining = ctx
        .mongo
        .quarantines
        .find_one(doc! {"guild_id": 107i64, "user_id": 37i64})
        .await
        .unwrap()
        .expect("active quarantine record should remain after permission failure");
    assert!(!remaining.released);
    assert_eq!(remaining.roles, vec![10]);
    let cached: Option<String> = redis_get(&ctx.redis, "spam:quarantine:107:37").await;
    assert_eq!(cached, Some("token".into()));
}
