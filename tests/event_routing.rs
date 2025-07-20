#![cfg(feature = "test-utils")]

mod utils;

use discord_bot::{
    dbs::mongo::models::{
        channel::{Channel, ChannelEnum},
        message::{Message, MessageEnum},
        role::{Role, RoleEnum},
    },
    events::message_delete,
};
use twilight_model::id::{Id, marker::GuildMarker};
use utils::{
    event::{message_delete as make_message_delete, message_delete_bulk},
    guild::{cache_guild, make_guild},
    mock_context::build_context,
    mock_http::{MessageOp, last_message},
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
    ctx.mongo.channels.insert_one(channel).await.unwrap();

    let role = Role {
        id: None,
        role_type: RoleEnum::Live,
        role_id: 20,
        guild_id: guild_id.get(),
        self_assignable: true,
    };
    ctx.mongo.roles.insert_one(role).await.unwrap();

    let record = Message {
        id: None,
        guild_id: guild_id.get(),
        channel_id: 10,
        message_id: 100,
        message_type: MessageEnum::Role,
    };
    ctx.mongo.messages.insert_one(record).await.unwrap();

    let event = make_message_delete(guild_id.get(), 10, 100);

    message_delete::handle_single(ctx.clone(), event).await;

    let record = last_message(&ctx.http).expect("message record");
    assert!(matches!(record.kind, MessageOp::Update | MessageOp::Create));
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
    ctx.mongo.channels.insert_one(channel).await.unwrap();

    let role = Role {
        id: None,
        role_type: RoleEnum::Live,
        role_id: 20,
        guild_id: guild_id.get(),
        self_assignable: true,
    };
    ctx.mongo.roles.insert_one(role).await.unwrap();

    let record = Message {
        id: None,
        guild_id: guild_id.get(),
        channel_id: 10,
        message_id: 100,
        message_type: MessageEnum::Role,
    };
    ctx.mongo.messages.insert_one(record).await.unwrap();

    let event = message_delete_bulk(guild_id.get(), 10, vec![50, 100]);

    message_delete::handle_bulk(ctx.clone(), event).await;

    let record = last_message(&ctx.http).expect("message record");
    assert!(matches!(record.kind, MessageOp::Update | MessageOp::Create));
}
