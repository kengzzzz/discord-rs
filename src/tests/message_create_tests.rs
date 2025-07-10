use crate::events::message_create::{build_ai_input, collect_attachments};
use twilight_model::{
    channel::{Attachment, Message, message::MessageType},
    id::Id,
    user::User,
    util::datetime::Timestamp,
};

fn dummy_attachment(id: u64) -> Attachment {
    Attachment {
        content_type: None,
        ephemeral: false,
        duration_secs: None,
        filename: format!("file{id}.txt"),
        flags: None,
        description: None,
        height: None,
        id: Id::new(id),
        proxy_url: String::new(),
        size: 1,
        title: None,
        url: format!("http://example.com/{id}"),
        waveform: None,
        width: None,
    }
}

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

fn basic_message(id: u64, attachments: Vec<Attachment>, ref_msg: Option<Message>) -> Message {
    Message {
        activity: None,
        application: None,
        application_id: None,
        attachments,
        author: dummy_user(10),
        call: None,
        channel_id: Id::new(1),
        components: Vec::new(),
        content: String::new(),
        edited_timestamp: None,
        embeds: Vec::new(),
        flags: None,
        guild_id: None,
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
        referenced_message: ref_msg.map(Box::new),
        role_subscription_data: None,
        sticker_items: Vec::new(),
        timestamp: Timestamp::from_secs(0).unwrap(),
        thread: None,
        tts: false,
        webhook_id: None,
    }
}

#[test]
fn test_build_ai_input() {
    let txt = build_ai_input("hi", Some("hello"));
    assert_eq!(txt, "Replying to: hello\nhi");

    let txt2 = build_ai_input("  ping  ", None);
    assert_eq!(txt2, "ping");
}

#[test]
fn test_collect_attachments() {
    let msg1_att = vec![dummy_attachment(1), dummy_attachment(2)];
    let ref_att = vec![dummy_attachment(3), dummy_attachment(4)];
    let referenced = basic_message(2, ref_att.clone(), None);
    let msg = basic_message(1, msg1_att.clone(), Some(referenced));
    let merged = collect_attachments(&msg);
    assert_eq!(merged.len(), 4);
    assert_eq!(merged[0].id.get(), 1);
    assert_eq!(merged[1].id.get(), 2);
    assert_eq!(merged[2].id.get(), 3);
    assert_eq!(merged[3].id.get(), 4);
}
