#![cfg(feature = "mock-redis")]

use mongodb::change_stream::event::{ChangeStreamEvent, OperationType};
use serde_json::json;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use twilight_model::application::interaction::Interaction;

mod utils;
use discord_bot;
use discord_bot::configs::CACHE_PREFIX;
use utils::context::test_context;
use utils::mock_db::{init_mock, spawn_watcher_mock};

#[tokio::test]
async fn test_ping_command_flow() {
    let ctx = Arc::new(test_context().await);

    let interaction_json = json!({
        "id": "2",
        "application_id": "1",
        "type": 2,
        "token": "tok",
        "guild_id": "1",
        "authorizing_integration_owners": {"0": null, "1": null},
        "entitlements": [],
        "user": {"id": "3", "username": "tester", "discriminator": "0001"},
        "data": {
            "id": "4",
            "name": "ping",
            "type": 1,
            "options": []
        }
    });
    let interaction: Interaction = serde_json::from_value(interaction_json).unwrap();

    discord_bot::events::interaction_create::handle(ctx, interaction).await;
}

#[tokio::test]
async fn test_channel_watcher_purge_cache() {
    let ctx = test_context().await;
    let pool = ctx.redis.clone();

    let key_channel = format!("{CACHE_PREFIX}:channel:{}", 123);
    let key_by_type = format!(
        "{CACHE_PREFIX}:channel-type:{}:{}",
        456,
        discord_bot::dbs::mongo::models::channel::ChannelEnum::Broadcast.value()
    );
    let key_list = format!(
        "{CACHE_PREFIX}:channels-by-type:{}",
        discord_bot::dbs::mongo::models::channel::ChannelEnum::Broadcast.value()
    );

    ctx.redis_set(&key_channel, &1u8).await;
    ctx.redis_set(&key_by_type, &1u8).await;
    ctx.redis_set(&key_list, &vec![1u8]).await;

    assert!(ctx.redis_get::<u8>(&key_channel).await.is_some());
    assert!(ctx.redis_get::<u8>(&key_by_type).await.is_some());
    assert!(ctx.redis_get::<Vec<u8>>(&key_list).await.is_some());

    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let token = CancellationToken::new();
    spawn_watcher_mock(
        &init_mock(),
        "channels",
        ReceiverStream::new(rx),
        move |evt: ChangeStreamEvent<discord_bot::dbs::mongo::models::channel::Channel>| {
            let pool = pool.clone();
            async move {
                if matches!(
                    evt.operation_type,
                    OperationType::Insert
                        | OperationType::Update
                        | OperationType::Replace
                        | OperationType::Delete
                ) {
                    if let Some(doc) = evt.full_document.or(evt.full_document_before_change) {
                        discord_bot::services::channel::ChannelService::purge_cache(
                            &pool,
                            doc.channel_id,
                        )
                        .await;
                        discord_bot::services::channel::ChannelService::purge_cache_by_type(
                            &pool,
                            doc.guild_id,
                            &doc.channel_type,
                        )
                        .await;
                        discord_bot::services::channel::ChannelService::purge_list_cache(
                            &pool,
                            &doc.channel_type,
                        )
                        .await;
                    }
                }
            }
        },
        token.clone(),
    )
    .await
    .unwrap();

    let evt_json = json!({
        "_id": {"_data": "token"},
        "operationType": "insert",
        "fullDocument": {"channel_type": "broadcast", "channel_id": 123, "guild_id": 456}
    });
    let evt: ChangeStreamEvent<discord_bot::dbs::mongo::models::channel::Channel> =
        serde_json::from_value(evt_json).unwrap();
    tx.send(Ok(evt)).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    token.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    assert!(ctx.redis_get::<u8>(&key_channel).await.is_none());
    assert!(ctx.redis_get::<u8>(&key_by_type).await.is_none());
    assert!(ctx.redis_get::<Vec<u8>>(&key_list).await.is_none());
}
