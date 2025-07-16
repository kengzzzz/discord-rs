use discord_bot::commands::{ai::AiCommand, verify::VerifyCommand};
use twilight_interactions::command::CommandModel;
use twilight_model::application::{
    command::CommandType,
    interaction::application_command::{CommandData, CommandDataOption, CommandOptionValue},
};
use twilight_model::id::{Id, marker::CommandMarker};

#[test]
fn test_verify_command_parse() {
    let data = CommandData {
        guild_id: None,
        id: Id::<CommandMarker>::new(1),
        name: "verify".into(),
        kind: CommandType::ChatInput,
        options: vec![CommandDataOption {
            name: "token".into(),
            value: CommandOptionValue::String("abc".into()),
        }],
        resolved: None,
        target_id: None,
    };
    let cmd = VerifyCommand::from_interaction(data.into()).unwrap();
    assert_eq!(cmd.token, "abc");
}

#[test]
fn test_ai_command_prompt_parse() {
    let data = CommandData {
        guild_id: None,
        id: Id::<CommandMarker>::new(2),
        name: "ai".into(),
        kind: CommandType::ChatInput,
        options: vec![CommandDataOption {
            name: "prompt".into(),
            value: CommandOptionValue::SubCommand(vec![CommandDataOption {
                name: "prompt".into(),
                value: CommandOptionValue::String("hello".into()),
            }]),
        }],
        resolved: None,
        target_id: None,
    };
    match AiCommand::from_interaction(data.into()).unwrap() {
        AiCommand::Prompt(p) => assert_eq!(p.prompt, "hello"),
        other => panic!("unexpected variant: {other:?}"),
    }
}

#[test]
fn test_ai_command_clear_parse() {
    let data = CommandData {
        guild_id: None,
        id: Id::<CommandMarker>::new(3),
        name: "ai".into(),
        kind: CommandType::ChatInput,
        options: vec![CommandDataOption {
            name: "clear".into(),
            value: CommandOptionValue::SubCommand(vec![]),
        }],
        resolved: None,
        target_id: None,
    };
    match AiCommand::from_interaction(data.into()).unwrap() {
        AiCommand::Clear(_) => (),
        other => panic!("unexpected variant: {other:?}"),
    }
}
