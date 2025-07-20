use super::*;
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

#[test]
fn test_not_found_embed_footer_and_title() {
    let guild = make_guild(Id::new(1), "guild");
    let cache = DefaultInMemoryCache::new();
    cache.update(&GuildCreate::Available(guild.clone()));
    let guild_ref = cache.guild(guild.id).expect("guild ref");
    let embed = MarketService::not_found_embed(&guild_ref).unwrap();
    assert_eq!(embed.title.as_deref(), Some("ไม่พบราคา"));
    assert_eq!(embed.footer.unwrap().text, "guild");
}

#[test]
fn test_error_embed_footer_and_title() {
    let guild = make_guild(Id::new(1), "guild");
    let cache = DefaultInMemoryCache::new();
    cache.update(&GuildCreate::Available(guild.clone()));
    let guild_ref = cache.guild(guild.id).expect("guild ref");
    let embed = MarketService::error_embed(&guild_ref).unwrap();
    assert_eq!(embed.title.as_deref(), Some("เกิดข้อผิดพลาด"));
    assert_eq!(embed.footer.unwrap().text, "guild");
}

#[test]
fn test_build_fields_limits_and_format() {
    let orders = (1..=6)
        .map(|i| session::OrderInfo {
            quantity: i,
            platinum: i * 10,
            ign: format!("User{i}"),
        })
        .collect::<Vec<_>>();
    let fields = MarketService::build_fields(&orders, "item", &MarketKind::Buy, Some(3));
    assert_eq!(fields.len(), 5);
    for (i, field) in fields.iter().enumerate() {
        let qty = i as u32 + 1;
        let plat = qty * 10;
        let name = format!("Quantity : {qty} | Price : {plat} platinum. [ Item Rank : 3 ]");
        let value = format!(
            "```/w User{qty} Hi! I want to buy: \"item\" for {plat} platinum. (warframe.market)```"
        );
        assert_eq!(field.name, name);
        assert_eq!(field.value, value);
    }
}

#[test]
fn test_build_embed_footer_title_url_and_fields() {
    let guild = make_guild(Id::new(1), "guild");
    let cache = DefaultInMemoryCache::new();
    cache.update(&GuildCreate::Available(guild.clone()));
    let guild_ref = cache.guild(guild.id).expect("guild ref");
    let orders = vec![
        session::OrderInfo {
            quantity: 1,
            platinum: 50,
            ign: "Tester".into(),
        },
        session::OrderInfo {
            quantity: 2,
            platinum: 60,
            ign: "Tester2".into(),
        },
    ];
    let embed = MarketService::build_embed(
        &guild_ref,
        "mod",
        "mod_url",
        &MarketKind::Sell,
        Some(1),
        orders.clone(),
    )
    .unwrap();
    assert_eq!(embed.footer.unwrap().text, "guild [ Item Rank : 1 ]");
    assert_eq!(embed.title.as_deref(), Some("ผู้ซื้อ mod [Rank 1]"));
    assert_eq!(
        embed.url.as_deref(),
        Some(format!("{}mod_url", client::ITEM_URL).as_str())
    );
    assert_eq!(embed.fields.len(), 2);
    let expected_fields = MarketService::build_fields(&orders, "mod", &MarketKind::Sell, Some(1));
    assert_eq!(embed.fields, expected_fields);
}
