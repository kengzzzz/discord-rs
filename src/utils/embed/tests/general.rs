use super::*;
use twilight_cache_inmemory::DefaultInMemoryCache;
use twilight_model::gateway::payload::incoming::GuildCreate;
use twilight_model::guild::{
    AfkTimeout, DefaultMessageNotificationLevel, ExplicitContentFilter, Guild, MfaLevel, NSFWLevel,
    PremiumTier, SystemChannelFlags, VerificationLevel,
};

fn make_guild(
    id: Id<GuildMarker>,
    name: &str,
    icon: Option<twilight_model::util::ImageHash>,
) -> Guild {
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
        icon,
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
fn test_footer_with_icon() {
    let icon_bytes = b"a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4";
    let icon = twilight_model::util::ImageHash::parse(icon_bytes).unwrap();
    let guild = make_guild(Id::new(1), "guild", Some(icon));
    let cache = DefaultInMemoryCache::new();
    cache.update(&GuildCreate::Available(guild.clone()));
    let guild_ref = cache
        .guild(guild.id)
        .expect("guild ref");

    let footer = footer_with_icon(&guild_ref).unwrap();
    let expected_url = format!(
        "https://cdn.discordapp.com/icons/{}/{}.png",
        guild.id, icon
    );
    assert_eq!(footer.text, guild.name);
    assert_eq!(
        footer.icon_url.as_deref(),
        Some(expected_url.as_str())
    );
}

#[test]
fn test_guild_only_embed() {
    let embed = guild_only_embed().unwrap();
    assert_eq!(
        embed.title.as_deref(),
        Some("This command can only be used in a server")
    );
    assert_eq!(embed.color, Some(COLOR_INVALID));
}

#[test]
fn test_pong_embed_latency_na() {
    let embed = pong_embed(None).unwrap();
    assert_eq!(embed.title.as_deref(), Some("Pong!"));
    assert_eq!(embed.description.as_deref(), Some("Latency: N/A"));
    assert_eq!(embed.color, Some(COLOR));
}
