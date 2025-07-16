use discord_bot::configs::Reaction;
use discord_bot::dbs::mongo::models::role::RoleEnum;
use discord_bot::services::ai::AiService;
mod util;
use discord_bot::utils::{env as env_util, reaction};
use discord_bot::{defer_interaction, send_with_fallback};
use twilight_model::channel::message::EmojiReactionType;
use twilight_model::id::{
    Id,
    marker::{ApplicationMarker, ChannelMarker, InteractionMarker, UserMarker},
};
use util::mock_http::MockHttp;

struct TestInteraction {
    application_id: Id<ApplicationMarker>,
    id: Id<InteractionMarker>,
    token: String,
}

#[tokio::test]
async fn test_env_parsing() {
    unsafe { std::env::set_var("TEST_NUM", "42") };
    assert_eq!(env_util::parse_env::<u32>("TEST_NUM", "0"), 42);
    unsafe { std::env::remove_var("TEST_NUM") };
    assert_eq!(env_util::parse_env::<u32>("TEST_NUM", "7"), 7);

    unsafe { std::env::set_var("TEST_OPT", "5") };
    assert_eq!(env_util::parse_env_opt::<u32>("TEST_OPT"), Some(5));
    unsafe { std::env::set_var("TEST_OPT", "") };
    assert_eq!(env_util::parse_env_opt::<u32>("TEST_OPT"), None);
    unsafe { std::env::remove_var("TEST_OPT") };
}

#[test]
fn test_reaction_maps() {
    let emoji = EmojiReactionType::Unicode {
        name: Reaction::Riven.emoji().into(),
    };
    assert_eq!(
        reaction::emoji_to_role_enum(&emoji),
        Some(RoleEnum::RivenSilver)
    );
    assert_eq!(
        reaction::role_enum_to_emoji(&RoleEnum::Helminth),
        Some(Reaction::Helminth.emoji())
    );
}

#[test]
fn test_ai_embeds_split() {
    let text = "a".repeat(4097);
    let embeds = AiService::ai_embeds(&text).unwrap();
    assert_eq!(embeds.len(), 5);
    assert_eq!(embeds[0].description.as_deref().unwrap().len(), 1024);
    assert_eq!(embeds[1].description.as_deref().unwrap().len(), 1024);
    assert_eq!(embeds[2].description.as_deref().unwrap().len(), 1024);
    assert_eq!(embeds[3].description.as_deref().unwrap().len(), 1024);
    assert_eq!(embeds[4].description.as_deref().unwrap().len(), 1);
    assert!(AiService::ai_embeds("").unwrap().is_empty());
}

#[tokio::test]
async fn test_defer_interaction_macro() {
    let http = MockHttp::new();
    http.clear();
    let interaction = TestInteraction {
        application_id: Id::new(1),
        id: Id::new(2),
        token: "tkn".into(),
    };

    defer_interaction!(&http, interaction, true).await.unwrap();

    let logs = http.logs.lock().unwrap().clone();
    assert_eq!(logs, vec!["create_response".to_string()]);
}

#[tokio::test]
async fn test_send_with_fallback_macro() {
    let mut http = MockHttp::new();
    http.clear();
    let http1 = http.clone();
    send_with_fallback!(
        &http1,
        Id::<UserMarker>::new(1),
        Id::<ChannelMarker>::new(9),
        |b| {
            b.content("hi").await?;
            Ok::<_, anyhow::Error>(())
        }
    );

    let logs = http.logs.lock().unwrap().clone();
    assert_eq!(
        logs,
        vec!["create_private_channel", "create_message"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );

    http.clear();
    http.dm_ok = false;
    let http2 = http.clone();
    send_with_fallback!(
        &http2,
        Id::<UserMarker>::new(1),
        Id::<ChannelMarker>::new(10),
        |b| {
            b.content("hi").await?;
            Ok::<_, anyhow::Error>(())
        }
    );
    let logs2 = http.logs.lock().unwrap().clone();
    assert_eq!(
        logs2,
        vec!["create_private_channel", "create_message"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
}
