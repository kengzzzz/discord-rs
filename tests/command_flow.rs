#![cfg(feature = "test-utils")]

mod utils;

use discord_bot::{
    commands::{
        ai::AiCommand, help::HelpCommand, intro::IntroCommand, ping::PingCommand,
        verify::VerifyCommand, warframe::WarframeCommand,
    },
    dbs::mongo::models::channel::{Channel, ChannelEnum},
    utils::embed,
};
use twilight_model::application::interaction::application_command::{
    CommandDataOption, CommandOptionValue,
};
use twilight_model::channel::message::MessageFlags;
use twilight_model::http::interaction::InteractionResponseType;
use twilight_model::id::Id;
use utils::{
    guild::{cache_guild, make_guild},
    interaction::{command_interaction, command_interaction_with_options},
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

#[tokio::test]
async fn help_command_embed() {
    let ctx = build_context().await;
    let guild = make_guild(Id::new(1), "guild");
    cache_guild(&ctx.cache, guild.clone());
    let (interaction, data) = command_interaction("help", Some(1));

    HelpCommand::handle(ctx.clone(), interaction, data).await;

    let record = last_message(&ctx.http).expect("message record");
    assert!(matches!(record.kind, MessageOp::Update));
    assert_eq!(record.embeds[0].title.as_deref(), Some("คำสั่งบอท"));

    let response = last_interaction(&ctx.http).expect("interaction record");
    assert_eq!(
        response.response.kind,
        InteractionResponseType::DeferredChannelMessageWithSource
    );
    assert_eq!(
        response.response.data.unwrap().flags,
        Some(MessageFlags::EPHEMERAL)
    );
}

#[tokio::test]
async fn help_command_dm_guild_only() {
    let ctx = build_context().await;
    let (interaction, data) = command_interaction("help", None);

    HelpCommand::handle(ctx.clone(), interaction, data).await;

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

#[tokio::test]
async fn intro_command_modal() {
    let ctx = build_context().await;
    let guild = make_guild(Id::new(1), "guild");
    cache_guild(&ctx.cache, guild.clone());
    let channel =
        Channel { id: None, channel_type: ChannelEnum::Introduction, channel_id: 1, guild_id: 1 };
    ctx.mongo
        .channels
        .insert_one(channel)
        .await
        .unwrap();
    let (interaction, data) = command_interaction("intro", Some(1));

    IntroCommand::handle(ctx.clone(), interaction, data).await;

    let response = last_interaction(&ctx.http).expect("interaction record");
    assert_eq!(
        response.response.kind,
        InteractionResponseType::Modal
    );
    let modal = response.response.data.expect("data");
    assert_eq!(modal.custom_id.as_deref(), Some("intro_modal"));
}

#[tokio::test]
async fn intro_command_dm_guild_only() {
    let ctx = build_context().await;
    let (interaction, data) = command_interaction("intro", None);

    IntroCommand::handle(ctx.clone(), interaction, data).await;

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

#[tokio::test]
async fn verify_command_modal() {
    let ctx = build_context().await;
    let guild = make_guild(Id::new(1), "guild");
    cache_guild(&ctx.cache, guild.clone());
    ctx.redis_set("spam:quarantine:1:200", &"token")
        .await;
    let (interaction, data) = command_interaction("verify", Some(1));

    VerifyCommand::handle(ctx.clone(), interaction, data).await;

    let response = last_interaction(&ctx.http).expect("interaction record");
    assert_eq!(
        response.response.kind,
        InteractionResponseType::Modal
    );
    let data = response.response.data.expect("data");
    assert_eq!(data.custom_id.as_deref(), Some("verify_modal"));
}

#[tokio::test]
async fn verify_command_dm_guild_only() {
    let ctx = build_context().await;
    let (interaction, data) = command_interaction("verify", None);

    VerifyCommand::handle(ctx.clone(), interaction, data).await;

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

#[tokio::test]
async fn warframe_build_command_embed() {
    let ctx = build_context().await;
    let guild = make_guild(Id::new(1), "guild");
    cache_guild(&ctx.cache, guild.clone());
    ctx.reqwest.add_json_response(
        "https://overframe.gg/api/v1/builds?item_name=test&author_id=10027&limit=5&sort_by=Score",
        "{\"results\":[{\"title\":\"Build\",\"url\":\"/b\",\"formas\":0,\"updated\":\"2023-01-01T00:00:00Z\",\"author\":{\"username\":\"t\",\"url\":\"/u\"}}]}"
    );
    let options = vec![CommandDataOption {
        name: "build".into(),
        value: CommandOptionValue::SubCommand(vec![CommandDataOption {
            name: "item".into(),
            value: CommandOptionValue::String("test".into()),
        }]),
    }];
    let (interaction, data) = command_interaction_with_options("warframe", Some(1), options);

    WarframeCommand::handle(ctx.clone(), interaction, data).await;

    let record = last_message(&ctx.http).expect("message record");
    assert!(matches!(record.kind, MessageOp::Update));
    assert!(!record.embeds.is_empty());

    let response = last_interaction(&ctx.http).expect("interaction record");
    assert_eq!(
        response.response.kind,
        InteractionResponseType::DeferredChannelMessageWithSource
    );
    assert_eq!(
        response.response.data.unwrap().flags,
        Some(MessageFlags::EPHEMERAL)
    );
}

#[tokio::test]
async fn warframe_command_dm_guild_only() {
    let ctx = build_context().await;
    let options = vec![CommandDataOption {
        name: "build".into(),
        value: CommandOptionValue::SubCommand(vec![CommandDataOption {
            name: "item".into(),
            value: CommandOptionValue::String("test".into()),
        }]),
    }];
    let (interaction, data) = command_interaction_with_options("warframe", None, options);

    WarframeCommand::handle(ctx.clone(), interaction, data).await;

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

#[tokio::test]
async fn ai_prompt_command_embed() {
    let ctx = build_context().await;
    let options = vec![CommandDataOption {
        name: "prompt".into(),
        value: CommandOptionValue::SubCommand(vec![CommandDataOption {
            name: "prompt".into(),
            value: CommandOptionValue::String("hello".into()),
        }]),
    }];
    let (interaction, data) = command_interaction_with_options("ai", Some(1), options);

    AiCommand::handle(ctx.clone(), interaction, data).await;

    let record = last_message(&ctx.http).expect("message record");
    assert!(matches!(record.kind, MessageOp::Update));
    assert!(!record.embeds.is_empty());

    let response = last_interaction(&ctx.http).expect("interaction record");
    assert_eq!(
        response.response.kind,
        InteractionResponseType::DeferredChannelMessageWithSource
    );
    assert_eq!(
        response.response.data.unwrap().flags,
        Some(MessageFlags::EPHEMERAL)
    );
}
