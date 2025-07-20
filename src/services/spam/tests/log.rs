use super::*;
use crate::context::ContextBuilder;
use crate::context::mock_http::MockClient as Client;
use twilight_model::{
    channel::{Attachment, message::MessageType},
    id::Id,
    user::User,
    util::datetime::Timestamp,
};

fn make_attachment(id: u64, name: &str, size: u64) -> Attachment {
    Attachment {
        content_type: None,
        ephemeral: false,
        duration_secs: None,
        filename: name.to_owned(),
        flags: None,
        description: None,
        height: None,
        id: Id::new(id),
        proxy_url: String::new(),
        size,
        title: None,
        url: String::new(),
        waveform: None,
        width: None,
    }
}

fn make_message(
    id: u64,
    channel_id: u64,
    guild_id: u64,
    user_id: u64,
    content: &str,
    attachments: Vec<Attachment>,
) -> Message {
    Message {
        activity: None,
        application: None,
        application_id: None,
        attachments,
        author: User {
            accent_color: None,
            avatar: None,
            avatar_decoration: None,
            avatar_decoration_data: None,
            banner: None,
            bot: false,
            discriminator: 0,
            email: None,
            flags: None,
            global_name: None,
            id: Id::new(user_id),
            locale: None,
            mfa_enabled: None,
            name: "tester".to_owned(),
            premium_type: None,
            public_flags: None,
            system: None,
            verified: None,
        },
        call: None,
        channel_id: Id::new(channel_id),
        components: Vec::new(),
        content: content.to_owned(),
        edited_timestamp: None,
        embeds: Vec::new(),
        flags: Some(twilight_model::channel::message::MessageFlags::empty()),
        guild_id: Some(Id::new(guild_id)),
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
        referenced_message: None,
        role_subscription_data: None,
        sticker_items: Vec::new(),
        timestamp: Timestamp::from_secs(1).unwrap(),
        thread: None,
        tts: false,
        webhook_id: None,
    }
}

async fn build_context() -> Arc<Context> {
    let ctx = ContextBuilder::new()
        .http(Client::new())
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
}

#[tokio::test]
async fn test_hash_message() {
    let att = make_attachment(1, "file.png", 10);
    let msg1 = make_message(1, 1, 1, 1, "hello", vec![att.clone()]);
    let msg2 = make_message(2, 1, 1, 1, "hello", vec![att.clone()]);
    let hash1 = hash_message(&msg1).await;
    let hash2 = hash_message(&msg2).await;
    assert_eq!(hash1, hash2);

    let msg3 = make_message(3, 1, 1, 1, "world", vec![att.clone()]);
    assert_ne!(hash1, hash_message(&msg3).await);

    let att2 = make_attachment(2, "other.png", 5);
    let msg4 = make_message(4, 1, 1, 1, "hello", vec![att, att2]);
    assert_ne!(hash1, hash_message(&msg4).await);
}

#[tokio::test]
async fn test_log_message_and_clear() {
    let ctx = build_context().await;

    for i in 1..SPAM_LIMIT as u64 {
        let msg = make_message(i, i, 1, 1, "spam", Vec::new());
        assert!(
            log_message(&ctx, 1, &msg)
                .await
                .is_none()
        );
    }
    let msg = make_message(99, SPAM_LIMIT as u64, 1, 1, "spam", Vec::new());
    assert!(
        log_message(&ctx, 1, &msg)
            .await
            .is_some()
    );

    let new_msg = make_message(100, 1, 1, 1, "different", Vec::new());
    log_message(&ctx, 1, &new_msg).await;
    let key = "spam:log:1:1";
    let record: SpamRecord = redis_get(&ctx.redis, key)
        .await
        .unwrap();
    assert_eq!(record.histories.len(), 1);

    clear_log(&ctx.redis, 1, 1).await;
    let none: Option<SpamRecord> = redis_get(&ctx.redis, key).await;
    assert!(none.is_none());
}
