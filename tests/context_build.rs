#![cfg(feature = "test-utils")]

mod utils;

use twilight_model::id::Id;
use utils::mock_context::build_context;

#[tokio::test]
async fn build_context_mocked_services() {
    let ctx = build_context().await;

    ctx.redis_set("test:key", &123).await;
    let val: Option<i32> = ctx.redis_get("test:key").await;
    assert_eq!(val, Some(123));

    use discord_bot::dbs::mongo::models::channel::{Channel, ChannelEnum};
    let channel = Channel {
        id: None,
        channel_type: ChannelEnum::None,
        channel_id: 1,
        guild_id: 1,
    };
    ctx.mongo.channels.insert_one(channel).await.unwrap();

    let _builder = ctx.http.create_message(Id::new(1));
}
