#![cfg(feature = "test-utils")]

mod utils;

use discord_bot::{commands::ping::PingCommand, utils::embed};
use twilight_model::channel::message::MessageFlags;
use twilight_model::http::interaction::InteractionResponseType;
use utils::{
    interaction::command_interaction,
    mock_context::build_context,
    mock_http::{MessageOp, last_interaction, last_message},
};

#[tokio::test]
async fn ping_command_sends_pong() {
    let ctx = build_context().await;
    let (interaction, data) = command_interaction("ping", Some(1));

    PingCommand::handle(ctx.clone(), interaction, data).await;

    let record = last_message(&ctx.http).expect("message record");
    assert!(matches!(record.kind, MessageOp::Update));
    assert_eq!(record.embeds.len(), 1);
    assert_eq!(record.embeds[0].title.as_deref(), Some("Pong!"));

    let response = last_interaction(&ctx.http).expect("interaction record");
    assert_eq!(
        response.response.kind,
        InteractionResponseType::DeferredChannelMessageWithSource
    );
    let data = response.response.data.unwrap();
    assert_eq!(data.flags, Some(MessageFlags::EPHEMERAL));
}

#[tokio::test]
async fn ping_command_dm_guild_only() {
    let ctx = build_context().await;
    let (interaction, data) = command_interaction("ping", None);

    PingCommand::handle(ctx.clone(), interaction, data).await;

    let response = last_interaction(&ctx.http).expect("interaction record");
    assert_eq!(
        response.response.kind,
        InteractionResponseType::ChannelMessageWithSource
    );
    let data = response.response.data.expect("data");
    assert_eq!(data.flags, Some(MessageFlags::EPHEMERAL));
    let embed = &data.embeds.expect("embed")[0];
    let expected = embed::guild_only_embed().unwrap();
    assert_eq!(embed.title, expected.title);
}
