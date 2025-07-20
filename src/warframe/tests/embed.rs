use super::*;
use crate::context::{ContextBuilder, mock_http::MockClient as Client};
use crate::warframe::api::{SteelPathData, SteelPathReward};
use std::sync::Arc;
use twilight_cache_inmemory::DefaultInMemoryCache;
use twilight_model::gateway::payload::incoming::GuildCreate;
use twilight_model::guild::{
    AfkTimeout, DefaultMessageNotificationLevel, ExplicitContentFilter, Guild, MfaLevel, NSFWLevel,
    PremiumTier, SystemChannelFlags, VerificationLevel,
};

fn make_guild(id: Id<GuildMarker>, name: &str) -> Guild {
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

async fn build_context() -> Arc<Context> {
    let ctx = ContextBuilder::new()
        .http(Client::new())
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
}

#[test]
fn test_ttl_from_expiry_minimum() {
    let expiry = (Utc::now() + chrono::Duration::seconds(10)).to_rfc3339();
    let ttl = ttl_from_expiry(&expiry);
    assert!(ttl >= MIN_CACHE_TTL);
}

#[test]
fn test_ttl_from_expiry_invalid() {
    assert_eq!(ttl_from_expiry("invalid"), MIN_CACHE_TTL);
}

#[tokio::test]
async fn test_steel_path_field_detects_umbra() {
    let ctx = build_context().await;
    let expiry = (Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
    let activation = Utc::now().to_rfc3339();
    let data = api::SteelPathData {
        current_reward: Some(api::SteelPathReward {
            name: "Umbra Forma Blueprint".into(),
        }),
        expiry: expiry.clone(),
        activation: Some(activation),
    };
    let key = format!("{CACHE_PREFIX}:wf:steel-path");
    redis_set_ex(&ctx.redis, &key, &data, 60).await;

    let (field, is_umbra) = steel_path_field(&ctx).await.unwrap();
    assert!(is_umbra);
    let expected_value = format!("**Umbra Forma Blueprint**\nends {}", format_time(&expiry));
    assert_eq!(field.value, expected_value);
}

#[tokio::test]
async fn test_status_embed_footer_and_fields() {
    let ctx = build_context().await;
    let exp = (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();

    let news_key = format!("{CACHE_PREFIX}:wf:news");
    let news = vec![api::NewsItem {
        image_link: Some("https://example.com/img.png".into()),
    }];
    redis_set_ex(&ctx.redis, &news_key, &news, 60).await;

    let steel_key = format!("{CACHE_PREFIX}:wf:steel-path");
    let steel = api::SteelPathData {
        current_reward: Some(api::SteelPathReward {
            name: "Umbra Forma Blueprint".into(),
        }),
        expiry: exp.clone(),
        activation: Some(Utc::now().to_rfc3339()),
    };
    redis_set_ex(&ctx.redis, &steel_key, &steel, 60).await;

    let cycle_endpoints = [
        ("earthCycle", "Earth"),
        ("cetusCycle", "Cetus"),
        ("vallisCycle", "Vallis"),
        ("cambionCycle", "Cambion"),
        ("zarimanCycle", "Zariman"),
    ];
    for (ep, _) in &cycle_endpoints {
        let key = format!("{CACHE_PREFIX}:wf:cycle:{ep}");
        let cycle = api::Cycle {
            state: "state".into(),
            expiry: exp.clone(),
        };
        redis_set_ex(&ctx.redis, &key, &cycle, 60).await;
    }

    let guild = make_guild(Id::new(1), "guild");
    let cache = DefaultInMemoryCache::new();
    cache.update(&GuildCreate::Available(guild.clone()));
    let guild_ref = cache.guild(guild.id).expect("guild ref");

    let (embed, is_umbra) = status_embed(&ctx, &guild_ref).await.unwrap();
    assert!(is_umbra);
    assert_eq!(embed.title.as_deref(), Some("[PC] Warframe Cycle Timers"));
    assert_eq!(embed.footer.unwrap().text, "guild");
    assert_eq!(embed.fields.len(), 6);
}

#[tokio::test]
async fn test_ttl_from_expiry_future() {
    let expiry = (Utc::now() + chrono::Duration::seconds(120)).to_rfc3339();
    assert!(ttl_from_expiry(&expiry) >= 120);
}

#[tokio::test]
async fn test_steel_path_field_umbra() {
    let ctx = Arc::new(ContextBuilder::new().watchers(false).build().await.unwrap());
    let data = SteelPathData {
        current_reward: Some(SteelPathReward {
            name: "Umbra Forma Blueprint".to_string(),
        }),
        expiry: (Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        activation: Some((Utc::now() - chrono::Duration::minutes(2)).to_rfc3339()),
    };
    let key = format!("{CACHE_PREFIX}:wf:steel-path");
    redis_set_ex(&ctx.redis, &key, &data, 60).await;

    let (_field, is_umbra) = steel_path_field(&ctx).await.unwrap();
    assert!(is_umbra);
}
