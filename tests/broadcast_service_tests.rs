#![allow(unused_imports)]

use std::sync::Arc;

use twilight_model::{
    channel::{Attachment, Message, message::MessageType},
    id::{Id, marker::GuildMarker},
    user::User,
    util::datetime::Timestamp,
};

use discord_bot::{configs::CACHE_PREFIX, services::broadcast::BroadcastService};

mod utils;
use utils::context::test_context;

fn dummy_user(id: u64) -> User {
    User {
        accent_color: None,
        avatar: None,
        avatar_decoration: None,
        avatar_decoration_data: None,
        banner: None,
        bot: false,
        discriminator: 1,
        email: None,
        flags: None,
        global_name: None,
        id: Id::new(id),
        locale: None,
        mfa_enabled: None,
        name: "tester".into(),
        premium_type: None,
        public_flags: None,
        system: None,
        verified: None,
    }
}

fn make_message(channel: u64, id: u64, user: u64, content: &str, att: Vec<Attachment>) -> Message {
    Message {
        activity: None,
        application: None,
        application_id: None,
        attachments: att,
        author: dummy_user(user),
        call: None,
        channel_id: Id::new(channel),
        components: Vec::new(),
        content: content.into(),
        edited_timestamp: None,
        embeds: Vec::new(),
        flags: None,
        guild_id: Some(Id::<GuildMarker>::new(1)),
        id: Id::new(id),
        #[allow(deprecated)]
        interaction: None,
        interaction_metadata: None,
        kind: MessageType::Regular,
        member: None,
        mention_channels: Vec::new(),
        mention_everyone: false,
        mention_roles: Vec::new(),
        mentions: Vec::new(),
        message_snapshots: Vec::new(),
        pinned: false,
        poll: None,
        reactions: Vec::new(),
        reference: None,
        referenced_message: None,
        role_subscription_data: None,
        sticker_items: Vec::new(),
        timestamp: Timestamp::from_secs(0).unwrap(),
        thread: None,
        tts: false,
        webhook_id: None,
    }
}

fn dummy_attachment(url: &str, kind: Option<&str>) -> Attachment {
    Attachment {
        content_type: kind.map(|s| s.to_string()),
        ephemeral: false,
        duration_secs: None,
        filename: "f".into(),
        flags: None,
        description: None,
        height: None,
        id: Id::new(1),
        proxy_url: url.into(),
        size: 1,
        title: None,
        url: url.into(),
        waveform: None,
        width: None,
    }
}

#[tokio::test]
async fn test_broadcast_embeds_no_attachment() {
    use twilight_cache_inmemory::DefaultInMemoryCache as InMemoryCache;
    use twilight_model::guild::{
        DefaultMessageNotificationLevel, ExplicitContentFilter, Guild, MfaLevel, NSFWLevel,
        PremiumTier, VerificationLevel,
    };

    let guild = Guild {
        afk_channel_id: None,
        afk_timeout: twilight_model::guild::AfkTimeout::FIFTEEN_MINUTES,
        application_id: None,
        approximate_member_count: None,
        approximate_presence_count: None,
        banner: None,
        channels: Vec::new(),
        default_message_notifications: DefaultMessageNotificationLevel::Mentions,
        description: None,
        discovery_splash: None,
        emojis: Vec::new(),
        explicit_content_filter: ExplicitContentFilter::AllMembers,
        features: Vec::new(),
        guild_scheduled_events: Vec::new(),
        icon: None,
        id: Id::new(1),
        joined_at: None,
        large: false,
        max_members: None,
        max_presences: None,
        max_stage_video_channel_users: None,
        max_video_channel_users: None,
        member_count: None,
        members: Vec::new(),
        mfa_level: MfaLevel::None,
        name: "Guild".into(),
        nsfw_level: NSFWLevel::Default,
        owner_id: Id::new(1),
        owner: None,
        permissions: None,
        preferred_locale: "en".into(),
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
        system_channel_flags: twilight_model::guild::SystemChannelFlags::empty(),
        system_channel_id: None,
        threads: Vec::new(),
        unavailable: None,
        vanity_url_code: None,
        verification_level: VerificationLevel::None,
        voice_states: Vec::new(),
        widget_channel_id: None,
        widget_enabled: None,
    };
    let cache = InMemoryCache::new();
    cache
        .update(&twilight_model::gateway::payload::incoming::GuildCreate::Available(guild.clone()));
    let guild_ref = cache.guild(guild.id).unwrap();
    let msg = make_message(1, 2, 3, "hello", Vec::new());
    let embeds = BroadcastService::broadcast_embeds(&guild_ref, &msg).unwrap();
    assert_eq!(embeds.len(), 1);
    assert_eq!(embeds[0].description.as_deref(), Some("hello"));
}

#[tokio::test]
async fn test_broadcast_embeds_with_image() {
    use twilight_cache_inmemory::DefaultInMemoryCache as InMemoryCache;
    use twilight_model::guild::{
        DefaultMessageNotificationLevel, ExplicitContentFilter, Guild, MfaLevel, NSFWLevel,
        PremiumTier, VerificationLevel,
    };

    let guild = Guild {
        afk_channel_id: None,
        afk_timeout: twilight_model::guild::AfkTimeout::FIFTEEN_MINUTES,
        application_id: None,
        approximate_member_count: None,
        approximate_presence_count: None,
        banner: None,
        channels: Vec::new(),
        default_message_notifications: DefaultMessageNotificationLevel::Mentions,
        description: None,
        discovery_splash: None,
        emojis: Vec::new(),
        explicit_content_filter: ExplicitContentFilter::AllMembers,
        features: Vec::new(),
        guild_scheduled_events: Vec::new(),
        icon: None,
        id: Id::new(1),
        joined_at: None,
        large: false,
        max_members: None,
        max_presences: None,
        max_stage_video_channel_users: None,
        max_video_channel_users: None,
        member_count: None,
        members: Vec::new(),
        mfa_level: MfaLevel::None,
        name: "Guild".into(),
        nsfw_level: NSFWLevel::Default,
        owner_id: Id::new(1),
        owner: None,
        permissions: None,
        preferred_locale: "en".into(),
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
        system_channel_flags: twilight_model::guild::SystemChannelFlags::empty(),
        system_channel_id: None,
        threads: Vec::new(),
        unavailable: None,
        vanity_url_code: None,
        verification_level: VerificationLevel::None,
        voice_states: Vec::new(),
        widget_channel_id: None,
        widget_enabled: None,
    };
    let cache = InMemoryCache::new();
    cache
        .update(&twilight_model::gateway::payload::incoming::GuildCreate::Available(guild.clone()));
    let guild_ref = cache.guild(guild.id).unwrap();
    let attachment = dummy_attachment("http://img", Some("image/png"));
    let msg = make_message(1, 2, 3, "hello", vec![attachment]);
    let embeds = BroadcastService::broadcast_embeds(&guild_ref, &msg).unwrap();
    assert_eq!(embeds.len(), 1);
    assert_eq!(embeds[0].description.as_deref(), Some("hello"));
    assert!(embeds[0].image.is_some());
}

#[tokio::test]
#[cfg(feature = "mock-redis")]
async fn test_delete_replicas() {
    let ctx = Arc::new(test_context().await);
    let key = format!("{CACHE_PREFIX}:broadcast:1");
    ctx.redis_set(&key, &vec![(2u64, 3u64)]).await;
    BroadcastService::delete_replicas(ctx.clone(), &[(0, 1)]).await;
    assert!(ctx.redis_get::<Vec<(u64, u64)>>(&key).await.is_none());
}
