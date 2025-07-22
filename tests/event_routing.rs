#![cfg(feature = "test-utils")]

mod utils;

use axum::http::StatusCode;
use discord_bot::{
    dbs::mongo::models::{
        channel::{Channel, ChannelEnum},
        message::{Message, MessageEnum},
        role::{Role, RoleEnum},
    },
    events::{interaction_create, message_create, message_delete, ready},
    services::health::HealthService,
};
use twilight_model::id::{Id, marker::GuildMarker};
use utils::{
    event::{
        make_message, message_delete as make_message_delete, message_delete_bulk, ready_event,
    },
    guild::{cache_guild, make_guild},
    interaction::command_interaction,
    mock_context::build_context,
    mock_http::{MessageOp, last_interaction, last_message},
};

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
        twilight_model::http::interaction::InteractionResponseType::DeferredChannelMessageWithSource
    );
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
async fn ready_event_sets_health_flags() {
    let ctx = build_context().await;
    let event = ready_event(1, &[1]);

    ready::handle(ctx.clone(), event).await;

    HealthService::set_mongo(true);

    let status = HealthService::health().await;
    assert_eq!(status, StatusCode::OK);
}
