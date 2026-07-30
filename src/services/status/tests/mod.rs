use super::*;
use crate::context::{
    ContextBuilder,
    mock_http::{MessageOp, MockClient},
};
use crate::dbs::mongo::models::channel::Channel;
use crate::dbs::mongo::models::message::{Message, MessageEnum};
use crate::dbs::redis::{redis_delete, redis_set_ex};
use crate::warframe::api::{Cycle, NewsItem, SteelPathData, SteelPathReward};
use chrono::Utc;
use mongodb::bson::{doc, to_bson};
use twilight_cache_inmemory::UpdateCache;
use twilight_model::gateway::payload::incoming::GuildCreate;
use twilight_model::guild::{
    AfkTimeout, DefaultMessageNotificationLevel, ExplicitContentFilter, Guild, MfaLevel, NSFWLevel,
    PremiumTier, SystemChannelFlags, VerificationLevel,
};

static STATUS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn make_guild(id: Id<twilight_model::id::marker::GuildMarker>, name: &str) -> Guild {
    Guild {
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
        id,
        joined_at: None,
        large: false,
        max_members: None,
        max_presences: None,
        max_stage_video_channel_users: None,
        max_video_channel_users: None,
        member_count: None,
        members: Vec::new(),
        mfa_level: MfaLevel::None,
        name: name.to_owned(),
        nsfw_level: NSFWLevel::Default,
        owner_id: Id::new(1),
        owner: None,
        permissions: None,
        preferred_locale: "en_us".to_owned(),
        premium_progress_bar_enabled: false,
        premium_subscription_count: None,
        premium_tier: PremiumTier::None,
        presences: Vec::new(),
        public_updates_channel_id: None,
        roles: Vec::new(),
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
    }
}

async fn seed_status_data(ctx: &Context) {
    let expiry = (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();
    redis_set_ex(
        &ctx.redis,
        "discord-bot:wf:news",
        &vec![NewsItem { image_link: None }],
        60,
    )
    .await;
    redis_set_ex(
        &ctx.redis,
        "discord-bot:wf:steel-path",
        &SteelPathData {
            current_reward: Some(SteelPathReward { name: "Reward".into() }),
            expiry: expiry.clone(),
            activation: None,
        },
        60,
    )
    .await;
    for endpoint in ["cetusCycle", "vallisCycle", "cambionCycle", "zarimanCycle"] {
        redis_set_ex(
            &ctx.redis,
            &format!("discord-bot:wf:cycle:{endpoint}"),
            &Cycle { state: "day".into(), expiry: expiry.clone() },
            60,
        )
        .await;
    }
}

async fn existing_status_context(guild_id: u64, channel_id: u64, message_id: u64) -> Arc<Context> {
    let ctx = Arc::new(
        ContextBuilder::new()
            .http(MockClient::new())
            .watchers(false)
            .build()
            .await
            .expect("build test context"),
    );
    ctx.cache
        .update(&GuildCreate::Available(make_guild(
            Id::new(guild_id),
            "guild",
        )));
    redis_delete(&ctx.redis, "discord-bot:channels-by-type:status").await;
    ctx.mongo
        .channels
        .insert_one(Channel { id: None, channel_type: ChannelEnum::Status, channel_id, guild_id })
        .await
        .expect("insert status channel");
    ctx.mongo
        .messages
        .insert_one(Message {
            id: None,
            guild_id,
            channel_id,
            message_id,
            message_type: MessageEnum::Status,
        })
        .await
        .expect("insert status message");
    seed_status_data(&ctx).await;
    ctx
}

fn message_op_count(ctx: &Context, matches: impl Fn(MessageOp) -> bool) -> usize {
    ctx.http
        .messages
        .lock()
        .unwrap()
        .iter()
        .filter(|record| matches(record.kind))
        .count()
}

#[tokio::test]
async fn steady_state_updates_without_get_preflight() {
    let _guard = STATUS_LOCK.lock().await;
    let ctx = existing_status_context(701, 702, 703).await;

    StatusService::update_all(&ctx).await;

    assert_eq!(ctx.http.update_message_call_count(), 1);
    assert_eq!(
        ctx.http.message_call_count(),
        0,
        "steady-state status update should PATCH directly"
    );
}

#[tokio::test]
async fn missing_message_is_recreated_after_patch() {
    let _guard = STATUS_LOCK.lock().await;
    let ctx = existing_status_context(711, 712, 713).await;
    ctx.http
        .fail_update_message_with(Id::new(713), 404, Some(10008));

    StatusService::update_all(&ctx).await;

    assert_eq!(ctx.http.update_message_call_count(), 1);
    assert_eq!(
        message_op_count(&ctx, |op| matches!(op, MessageOp::Create)),
        1,
        "Discord code 10008 should create a replacement status message"
    );
    let replacement_id = ctx
        .http
        .messages
        .lock()
        .unwrap()
        .iter()
        .find(|record| matches!(record.kind, MessageOp::Create))
        .expect("replacement status message")
        .message_id;

    StatusService::update_all(&ctx).await;

    assert_eq!(ctx.http.update_message_call_count(), 2);
    assert_eq!(
        message_op_count(&ctx, |op| matches!(op, MessageOp::Create)),
        1,
        "the replacement ID should be cached for the next update"
    );
    assert!(
        ctx.http
            .messages
            .lock()
            .unwrap()
            .iter()
            .any(|record| {
                matches!(record.kind, MessageOp::Update) && record.message_id == replacement_id
            }),
        "the next pass should PATCH the replacement message"
    );
}

#[tokio::test]
async fn transient_patch_error_keeps_existing_message_record() {
    let _guard = STATUS_LOCK.lock().await;
    let ctx = existing_status_context(721, 722, 723).await;
    ctx.http
        .fail_update_message_with(Id::new(723), 503, None);

    StatusService::update_all(&ctx).await;

    assert_eq!(ctx.http.update_message_call_count(), 1);
    assert_eq!(
        message_op_count(&ctx, |op| matches!(op, MessageOp::Create)),
        0,
        "transient PATCH errors must not create duplicate messages"
    );
    let stored = ctx
        .mongo
        .messages
        .find_one(doc! {
            "guild_id": 721_i64,
            "message_type": to_bson(&MessageEnum::Status).unwrap(),
        })
        .await
        .unwrap()
        .expect("existing status record");
    assert_eq!(stored.message_id, 723);
}

async fn add_status_channel(ctx: &Context, guild_id: u64, channel_id: u64, name: &str) {
    ctx.cache
        .update(&GuildCreate::Available(make_guild(
            Id::new(guild_id),
            name,
        )));
    ctx.mongo
        .channels
        .insert_one(Channel { id: None, channel_type: ChannelEnum::Status, channel_id, guild_id })
        .await
        .expect("insert status channel");
}

async fn clear_status_data(ctx: &Context) {
    for key in [
        "discord-bot:wf:news",
        "discord-bot:wf:steel-path",
        "discord-bot:wf:cycle:cetusCycle",
        "discord-bot:wf:cycle:vallisCycle",
        "discord-bot:wf:cycle:cambionCycle",
        "discord-bot:wf:cycle:zarimanCycle",
        "discord-bot:channels-by-type:status",
    ] {
        redis_delete(&ctx.redis, key).await;
    }
}

#[tokio::test]
async fn two_channels_share_one_failed_upstream_attempt() {
    let _guard = STATUS_LOCK.lock().await;
    let ctx = Arc::new(
        ContextBuilder::new()
            .http(MockClient::new())
            .watchers(false)
            .build()
            .await
            .expect("build test context"),
    );
    clear_status_data(&ctx).await;
    add_status_channel(&ctx, 731, 732, "guild one").await;
    add_status_channel(&ctx, 733, 734, "guild two").await;

    let expiry = (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();
    ctx.reqwest.add_json_response(
        "https://api.warframestat.us/pc/news",
        r#"[{"imageLink":null}]"#,
    );
    ctx.reqwest.add_json_response(
        "https://api.warframestat.us/pc/steelPath",
        &serde_json::to_string(&SteelPathData {
            current_reward: None,
            expiry: expiry.clone(),
            activation: None,
        })
        .unwrap(),
    );
    ctx.reqwest.add_json_response(
        "https://api.warframestat.us/pc/cetusCycle",
        "null",
    );
    for endpoint in ["vallisCycle", "cambionCycle", "zarimanCycle"] {
        ctx.reqwest.add_json_response(
            &format!("https://api.warframestat.us/pc/{endpoint}"),
            &serde_json::to_string(&Cycle { state: "day".into(), expiry: expiry.clone() }).unwrap(),
        );
    }

    StatusService::update_all(&ctx).await;

    assert_eq!(
        ctx.reqwest
            .json_request_count("https://api.warframestat.us/pc/cetusCycle"),
        1,
        "one update pass should fetch one shared status snapshot"
    );
}
