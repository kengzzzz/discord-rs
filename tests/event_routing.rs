#![cfg(feature = "test-utils")]

mod utils;

use async_trait::async_trait;
use axum::http::StatusCode;
use discord_bot::{
    configs::CACHE_PREFIX,
    configs::scam_detect::ScamDetectConfig,
    dbs::mongo::models::{
        channel::{Channel, ChannelEnum},
        message::{Message, MessageEnum},
        role::{Role, RoleEnum},
    },
    events::{interaction_create, message_create, message_delete, ready},
    services::health::HealthService,
    services::{
        guild_settings::GuildSettingsService,
        scam_detect::{ImageSize, ScamDetectQueue, ScamDetector, ScanResponse},
        spam::log,
    },
};
use std::{sync::Arc, time::Duration};
use tokio::task::yield_now;
use twilight_model::application::interaction::application_command::{
    CommandDataOption, CommandOptionValue,
};
use twilight_model::channel::Attachment;
use twilight_model::guild::{Member, MemberFlags};
use twilight_model::http::interaction::InteractionResponseType;
use twilight_model::id::{
    Id,
    marker::{GuildMarker, RoleMarker},
};
use utils::{
    event::{
        make_message, message_delete as make_message_delete, message_delete_bulk, ready_event,
    },
    guild::{cache_guild, make_guild},
    interaction::{
        autocomplete_interaction_with_options, command_interaction, focused_string_option,
    },
    mock_context::build_context,
    mock_http::{MessageOp, last_interaction, last_message},
};

fn image_attachment(id: u64, name: &str, size: u64, width: u64, height: u64) -> Attachment {
    Attachment {
        content_type: Some("image/png".to_owned()),
        ephemeral: false,
        duration_secs: None,
        filename: name.to_owned(),
        flags: None,
        description: None,
        height: Some(height),
        id: Id::new(id),
        proxy_url: String::new(),
        size,
        title: None,
        url: String::new(),
        waveform: None,
        width: Some(width),
    }
}

struct FakeScamDetector {
    result: anyhow::Result<ScanResponse>,
}

#[async_trait]
impl ScamDetector for FakeScamDetector {
    async fn scan(&self, _attachment: &Attachment) -> anyhow::Result<ScanResponse> {
        match &self.result {
            Ok(response) => Ok(response.clone()),
            Err(error) => Err(anyhow::anyhow!(error.to_string())),
        }
    }
}

fn scam_detect_config() -> Arc<ScamDetectConfig> {
    Arc::new(ScamDetectConfig {
        url: Some("http://detector.test".to_owned()),
        token: None,
        queue_capacity: 8,
        workers: 2,
        max_images_per_message: 3,
        max_upload_mb: 10,
        download_timeout: Duration::from_secs(1),
        scan_timeout: Duration::from_secs(1),
        job_ttl: Duration::from_secs(30),
    })
}

fn scan_response(action: &str, is_spam: bool) -> ScanResponse {
    ScanResponse {
        is_spam,
        risk: if is_spam { 0.91 } else { 0.2 },
        action: action.to_owned(),
        score_raw: if is_spam { 18 } else { 2 },
        reasons: vec!["withdrawal success".to_owned()],
        ocr_text: String::new(),
        ocr_text_length: 0,
        processing_ms: 1,
        image_size: ImageSize { width: 1280, height: 720 },
    }
}

fn embed_field<'a>(
    embed: &'a twilight_model::channel::message::Embed,
    name: &str,
) -> Option<&'a str> {
    embed
        .fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
}

async fn build_context_with_detector(
    result: anyhow::Result<ScanResponse>,
) -> Arc<discord_bot::context::Context> {
    let detector = Arc::new(FakeScamDetector { result });
    let queue = ScamDetectQueue::with_detector(scam_detect_config(), detector);
    let ctx = discord_bot::context::ContextBuilder::new()
        .http(discord_bot::context::mock_http::MockClient::new())
        .scam_detect(queue)
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
}

#[tokio::test]
async fn message_delete_triggers_role_message_update() {
    let ctx = build_context().await;
    let guild_id = Id::<GuildMarker>::new(1);

    let guild = make_guild(guild_id, "guild");
    cache_guild(&ctx.cache, guild);

    let channel = Channel {
        id: None,
        channel_type: ChannelEnum::UpdateRole,
        channel_id: 10,
        guild_id: guild_id.get(),
    };
    ctx.mongo
        .channels
        .insert_one(channel)
        .await
        .unwrap();

    let role = Role {
        id: None,
        role_type: RoleEnum::Live,
        role_id: 20,
        guild_id: guild_id.get(),
        self_assignable: true,
    };
    ctx.mongo
        .roles
        .insert_one(role)
        .await
        .unwrap();

    let record = Message {
        id: None,
        guild_id: guild_id.get(),
        channel_id: 10,
        message_id: 100,
        message_type: MessageEnum::Role,
    };
    ctx.mongo
        .messages
        .insert_one(record)
        .await
        .unwrap();

    let event = make_message_delete(guild_id.get(), 10, 100);

    message_delete::handle_single(ctx.clone(), event).await;

    let record = last_message(&ctx.http).expect("message record");
    assert!(matches!(
        record.kind,
        MessageOp::Update | MessageOp::Create
    ));
}

#[tokio::test]
async fn message_delete_bulk_triggers_role_message_update() {
    let ctx = build_context().await;
    let guild_id = Id::<GuildMarker>::new(1);

    let guild = make_guild(guild_id, "guild");
    cache_guild(&ctx.cache, guild);

    let channel = Channel {
        id: None,
        channel_type: ChannelEnum::UpdateRole,
        channel_id: 10,
        guild_id: guild_id.get(),
    };
    ctx.mongo
        .channels
        .insert_one(channel)
        .await
        .unwrap();

    let role = Role {
        id: None,
        role_type: RoleEnum::Live,
        role_id: 20,
        guild_id: guild_id.get(),
        self_assignable: true,
    };
    ctx.mongo
        .roles
        .insert_one(role)
        .await
        .unwrap();

    let record = Message {
        id: None,
        guild_id: guild_id.get(),
        channel_id: 10,
        message_id: 100,
        message_type: MessageEnum::Role,
    };
    ctx.mongo
        .messages
        .insert_one(record)
        .await
        .unwrap();

    let event = message_delete_bulk(guild_id.get(), 10, vec![50, 100]);

    message_delete::handle_bulk(ctx.clone(), event).await;

    let record = last_message(&ctx.http).expect("message record");
    assert!(matches!(
        record.kind,
        MessageOp::Update | MessageOp::Create
    ));
}

#[tokio::test]
async fn interaction_routing_dispatches_ping() {
    let ctx = build_context().await;
    let ready = ready_event(1, &[1]);
    ready::handle(ctx.clone(), ready).await;
    let (interaction, _data) = command_interaction("ping", Some(1));

    interaction_create::handle(ctx.clone(), interaction).await;

    let record = last_message(&ctx.http).expect("message record");
    assert!(matches!(record.kind, MessageOp::Update));

    let response = last_interaction(&ctx.http).expect("interaction record");
    assert_eq!(
        response.response.kind,
        InteractionResponseType::DeferredChannelMessageWithSource
    );
}

#[tokio::test]
async fn interaction_routing_dispatches_warframe_market_autocomplete() {
    let ctx = build_context().await;
    ctx.reqwest.add_json_response(
        "https://api.warframe.market/v2/items",
        "{\"data\":[{\"id\":\"test-id\",\"slug\":\"test_item\",\"i18n\":{\"en\":{\"name\":\"Test Item\"}}}]}",
    );
    discord_bot::services::market::MarketService::init(ctx.clone()).await;

    let options = vec![CommandDataOption {
        name: "market".into(),
        value: CommandOptionValue::SubCommand(vec![
            focused_string_option("item", "Te"),
            CommandDataOption {
                name: "kind".into(),
                value: CommandOptionValue::String("buy".into()),
            },
        ]),
    }];
    let (interaction, _data) = autocomplete_interaction_with_options("warframe", Some(1), options);

    interaction_create::handle(ctx.clone(), interaction).await;

    let response = last_interaction(&ctx.http).expect("interaction record");
    assert_eq!(
        response.response.kind,
        InteractionResponseType::ApplicationCommandAutocompleteResult
    );
    let choices = response
        .response
        .data
        .expect("response data")
        .choices
        .expect("autocomplete choices");
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0].name, "Test Item");
}

#[tokio::test]
async fn message_create_broadcasts() {
    let ctx = build_context().await;
    let guild_id = Id::<GuildMarker>::new(1);
    let guild = make_guild(guild_id, "guild");
    cache_guild(&ctx.cache, guild);

    let ch_src = Channel {
        id: None,
        channel_type: ChannelEnum::Broadcast,
        channel_id: 10,
        guild_id: guild_id.get(),
    };
    let ch_dest = Channel {
        id: None,
        channel_type: ChannelEnum::Broadcast,
        channel_id: 11,
        guild_id: guild_id.get(),
    };
    ctx.mongo
        .channels
        .insert_one(ch_src)
        .await
        .unwrap();
    ctx.mongo
        .channels
        .insert_one(ch_dest)
        .await
        .unwrap();

    let message = make_message(1, 10, Some(guild_id.get()), 200, "hello");

    message_create::handle(ctx.clone(), message).await;

    let record = last_message(&ctx.http).expect("message record");
    assert_eq!(record.channel_id.get(), 11);
    assert!(matches!(record.kind, MessageOp::Create));
}

#[tokio::test]
async fn message_create_broadcast_group_isolated() {
    let ctx = build_context().await;
    let guild_id = Id::<GuildMarker>::new(1);
    let guild = make_guild(guild_id, "guild");
    cache_guild(&ctx.cache, guild);

    let ch_src = Channel {
        id: None,
        channel_type: ChannelEnum::BroadcastB1,
        channel_id: 10,
        guild_id: guild_id.get(),
    };
    let ch_dest_same = Channel {
        id: None,
        channel_type: ChannelEnum::BroadcastB1,
        channel_id: 11,
        guild_id: guild_id.get(),
    };
    let ch_dest_other = Channel {
        id: None,
        channel_type: ChannelEnum::BroadcastB2,
        channel_id: 12,
        guild_id: guild_id.get(),
    };
    ctx.mongo
        .channels
        .insert_one(ch_src)
        .await
        .unwrap();
    ctx.mongo
        .channels
        .insert_one(ch_dest_same)
        .await
        .unwrap();
    ctx.mongo
        .channels
        .insert_one(ch_dest_other)
        .await
        .unwrap();

    let message = make_message(1, 10, Some(guild_id.get()), 200, "hello");

    message_create::handle(ctx.clone(), message).await;

    let messages = ctx.http.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    let record = messages.last().expect("message record");
    assert_eq!(record.channel_id.get(), 11);
    assert!(matches!(record.kind, MessageOp::Create));
}

#[tokio::test]
async fn message_create_quarantine_skips_other_routes() {
    let ctx = build_context().await;
    let guild_id = Id::<GuildMarker>::new(1);
    let guild = make_guild(guild_id, "guild");
    cache_guild(&ctx.cache, guild);

    let q_channel = Channel {
        id: None,
        channel_type: ChannelEnum::Quarantine,
        channel_id: 99,
        guild_id: guild_id.get(),
    };
    ctx.mongo
        .channels
        .insert_one(q_channel)
        .await
        .unwrap();

    let q_role = Role {
        id: None,
        role_type: RoleEnum::Quarantine,
        role_id: 55,
        guild_id: guild_id.get(),
        self_assignable: false,
    };
    ctx.mongo
        .roles
        .insert_one(q_role)
        .await
        .unwrap();

    ctx.redis_set(
        &format!("spam:quarantine:{}:{}", guild_id.get(), 200),
        &"tok",
    )
    .await;

    let msg = make_message(2, 10, Some(guild_id.get()), 200, "bad");

    message_create::handle(ctx.clone(), msg).await;

    let record = last_message(&ctx.http).expect("message record");
    assert_eq!(record.channel_id.get(), 99);
    assert!(matches!(record.kind, MessageOp::Create));
}

#[tokio::test]
async fn message_create_campaign_quarantine_clears_broadcast_replicas() {
    let ctx = build_context().await;
    let guild_id = Id::<GuildMarker>::new(1);
    let guild = make_guild(guild_id, "guild");
    cache_guild(&ctx.cache, guild);

    let q_channel = Channel {
        id: None,
        channel_type: ChannelEnum::Quarantine,
        channel_id: 99,
        guild_id: guild_id.get(),
    };
    ctx.mongo
        .channels
        .insert_one(q_channel)
        .await
        .unwrap();

    let q_role = Role {
        id: None,
        role_type: RoleEnum::Quarantine,
        role_id: 55,
        guild_id: guild_id.get(),
        self_assignable: false,
    };
    ctx.mongo
        .roles
        .insert_one(q_role)
        .await
        .unwrap();

    for (message_id, channel_id, suffix) in
        [(1001_u64, 10_u64, "111111"), (1002_u64, 11_u64, "222222"), (1003_u64, 12_u64, "333333")]
    {
        let mut seeded = make_message(
            message_id,
            channel_id,
            Some(guild_id.get()),
            200,
            &format!("check this https://discord.com/invite/{suffix}"),
        );
        seeded.attachments = vec![
            image_attachment(
                message_id * 10 + 1,
                "a.png",
                10 + channel_id,
                100 + channel_id,
                200 + channel_id,
            ),
            image_attachment(
                message_id * 10 + 2,
                "b.png",
                20 + channel_id,
                300 + channel_id,
                400 + channel_id,
            ),
            image_attachment(
                message_id * 10 + 3,
                "c.png",
                30 + channel_id,
                500 + channel_id,
                600 + channel_id,
            ),
        ];

        assert!(matches!(
            log::log_message(&ctx, guild_id.get(), &seeded).await,
            log::LogOutcome::None
        ));
    }

    ctx.redis_set(
        &format!("{CACHE_PREFIX}:broadcast:1002"),
        &vec![(77_u64, 8800_u64)],
    )
    .await;

    let mut trigger = make_message(
        1004,
        13,
        Some(guild_id.get()),
        200,
        "check this https://discord.com/invite/444444",
    );
    trigger.attachments = vec![
        image_attachment(2001, "d.png", 99, 150, 250),
        image_attachment(2002, "e.png", 109, 350, 450),
        image_attachment(2003, "f.png", 119, 550, 650),
    ];

    message_create::handle(ctx.clone(), trigger).await;

    for _ in 0..5 {
        yield_now().await;
    }

    let records = ctx
        .http
        .messages
        .lock()
        .unwrap()
        .clone();
    let deletes: Vec<_> = records
        .iter()
        .filter(|record| matches!(record.kind, MessageOp::Delete))
        .map(|record| (record.channel_id.get(), record.message_id.get()))
        .collect();

    assert!(deletes.contains(&(10, 1001)));
    assert!(deletes.contains(&(11, 1002)));
    assert!(deletes.contains(&(12, 1003)));
    assert!(deletes.contains(&(13, 1004)));
    assert!(deletes.contains(&(77, 8800)));

    let quarantine_notice = records
        .iter()
        .find(|record| {
            matches!(record.kind, MessageOp::Create)
                && record.channel_id.get() == 99
                && record
                    .embeds
                    .first()
                    .and_then(|embed| embed.title.as_deref())
                    == Some("ตรวจพบไฟล์แนบต้องสงสัย")
        })
        .expect("quarantine notice");
    assert_eq!(quarantine_notice.channel_id.get(), 99);
    assert_eq!(quarantine_notice.embeds.len(), 1);
    let embed = &quarantine_notice.embeds[0];
    assert_eq!(
        embed.title.as_deref(),
        Some("ตรวจพบไฟล์แนบต้องสงสัย")
    );
    assert!(embed.image.is_none());
    assert_eq!(embed_field(embed, "เหตุผล"), Some("ไฟล์แนบรูปภาพ"));
    assert!(
        embed_field(embed, "ตัวอย่างข้อความ")
            .expect("message preview")
            .contains("discord.com/invite/444444")
    );
    assert!(
        embed_field(embed, "ไฟล์แนบ")
            .expect("attachment summary")
            .contains("d.png")
    );
}

#[tokio::test]
async fn message_create_scam_image_quarantines_in_background() {
    let ctx = build_context_with_detector(Ok(scan_response("block", true))).await;
    let guild_id = Id::<GuildMarker>::new(1);
    let mut guild = make_guild(guild_id, "guild");
    let mut msg = make_message(5001, 10, Some(guild_id.get()), 240, "");
    guild.members.push(Member {
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
        roles: vec![Id::<RoleMarker>::new(77)],
        user: msg.author.clone(),
    });
    cache_guild(&ctx.cache, guild);

    ctx.mongo
        .channels
        .insert_one(Channel {
            id: None,
            channel_type: ChannelEnum::Quarantine,
            channel_id: 99,
            guild_id: guild_id.get(),
        })
        .await
        .unwrap();
    ctx.mongo
        .roles
        .insert_one(Role {
            id: None,
            role_type: RoleEnum::Quarantine,
            role_id: 55,
            guild_id: guild_id.get(),
            self_assignable: false,
        })
        .await
        .unwrap();
    GuildSettingsService::set_scam_detect_enabled(&ctx, guild_id.get(), true)
        .await
        .unwrap();

    let mut attachment = image_attachment(1, "scam.png", 1024, 640, 360);
    attachment.url = "https://cdn.example/scam.png".to_owned();
    msg.attachments = vec![attachment];

    message_create::handle(ctx.clone(), msg).await;

    for _ in 0..20 {
        yield_now().await;
    }

    let records = ctx
        .http
        .messages
        .lock()
        .unwrap()
        .clone();
    assert!(records.iter().any(|record| {
        matches!(record.kind, MessageOp::Delete)
            && record.channel_id.get() == 10
            && record.message_id.get() == 5001
    }));
    assert!(records.iter().any(|record| {
        matches!(record.kind, MessageOp::Create) && record.channel_id.get() == 99
    }));
    let quarantine_notice = records
        .iter()
        .find(|record| {
            matches!(record.kind, MessageOp::Create)
                && record.channel_id.get() == 99
                && record
                    .embeds
                    .first()
                    .and_then(|embed| embed.title.as_deref())
                    == Some("ตรวจพบไฟล์แนบต้องสงสัย")
        })
        .expect("quarantine notice");
    assert_eq!(quarantine_notice.embeds.len(), 1);
    let embed = &quarantine_notice.embeds[0];
    assert_eq!(
        embed.title.as_deref(),
        Some("ตรวจพบไฟล์แนบต้องสงสัย")
    );
    assert!(embed.image.is_none());
    assert!(
        embed_field(embed, "ไฟล์แนบ")
            .expect("attachment summary")
            .contains("scam.png")
    );

    let stored: Option<String> = ctx
        .redis_get("spam:quarantine:1:240")
        .await;
    assert!(stored.is_some());
}

#[tokio::test]
async fn message_create_scam_detector_unavailable_fails_open() {
    let ctx = build_context_with_detector(Err(anyhow::anyhow!("service unavailable"))).await;
    let guild_id = Id::<GuildMarker>::new(1);
    let guild = make_guild(guild_id, "guild");
    cache_guild(&ctx.cache, guild);

    ctx.mongo
        .channels
        .insert_one(Channel {
            id: None,
            channel_type: ChannelEnum::Quarantine,
            channel_id: 99,
            guild_id: guild_id.get(),
        })
        .await
        .unwrap();
    ctx.mongo
        .roles
        .insert_one(Role {
            id: None,
            role_type: RoleEnum::Quarantine,
            role_id: 55,
            guild_id: guild_id.get(),
            self_assignable: false,
        })
        .await
        .unwrap();
    GuildSettingsService::set_scam_detect_enabled(&ctx, guild_id.get(), true)
        .await
        .unwrap();

    let mut msg = make_message(5002, 10, Some(guild_id.get()), 201, "");
    let mut attachment = image_attachment(2, "maybe.png", 1024, 640, 360);
    attachment.url = "https://cdn.example/maybe.png".to_owned();
    msg.attachments = vec![attachment];

    message_create::handle(ctx.clone(), msg).await;

    for _ in 0..20 {
        yield_now().await;
    }

    let records = ctx
        .http
        .messages
        .lock()
        .unwrap()
        .clone();
    assert!(
        !records
            .iter()
            .any(|record| matches!(record.kind, MessageOp::Delete))
    );
    let stored: Option<String> = ctx
        .redis_get("spam:quarantine:1:201")
        .await;
    assert!(stored.is_none());
}

#[tokio::test]
async fn message_create_scam_detect_disabled_by_default() {
    let ctx = build_context_with_detector(Ok(scan_response("block", true))).await;
    let guild_id = Id::<GuildMarker>::new(1);
    let guild = make_guild(guild_id, "guild");
    cache_guild(&ctx.cache, guild);

    ctx.mongo
        .channels
        .insert_one(Channel {
            id: None,
            channel_type: ChannelEnum::Quarantine,
            channel_id: 99,
            guild_id: guild_id.get(),
        })
        .await
        .unwrap();
    ctx.mongo
        .roles
        .insert_one(Role {
            id: None,
            role_type: RoleEnum::Quarantine,
            role_id: 55,
            guild_id: guild_id.get(),
            self_assignable: false,
        })
        .await
        .unwrap();

    let mut msg = make_message(5003, 10, Some(guild_id.get()), 202, "");
    let mut attachment = image_attachment(3, "scam.png", 1024, 640, 360);
    attachment.url = "https://cdn.example/scam.png".to_owned();
    msg.attachments = vec![attachment];

    message_create::handle(ctx.clone(), msg).await;

    for _ in 0..20 {
        yield_now().await;
    }

    let records = ctx
        .http
        .messages
        .lock()
        .unwrap()
        .clone();
    assert!(
        !records
            .iter()
            .any(|record| matches!(record.kind, MessageOp::Delete))
    );
    let stored: Option<String> = ctx
        .redis_get("spam:quarantine:1:202")
        .await;
    assert!(stored.is_none());
}

#[tokio::test]
async fn ready_event_sets_health_flags() {
    let ctx = build_context().await;
    let event = ready_event(1, &[1]);

    ready::handle(ctx.clone(), event).await;

    HealthService::set_mongo(true);

    let status = HealthService::health().await;
    assert_eq!(status, StatusCode::OK);
}
