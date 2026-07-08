use super::*;
use crate::context::ContextBuilder;
use crate::context::mock_http::MockClient as Client;
use mongodb::bson::doc;
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

fn make_image_attachment(id: u64, name: &str, size: u64, width: u64, height: u64) -> Attachment {
    let mut attachment = make_attachment(id, name, size);
    attachment.content_type = Some("image/png".to_owned());
    attachment.width = Some(width);
    attachment.height = Some(height);
    attachment
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
            primary_guild: None,
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

async fn reset_spam_state(ctx: &Arc<Context>, guild_id: u64, user_id: u64) {
    clear_log(&ctx.redis, guild_id, user_id).await;
    quarantine::purge_cache(&ctx.redis, guild_id, user_id).await;
    ctx.mongo
        .quarantines
        .delete_many(doc! {"guild_id": guild_id as i64, "user_id": user_id as i64})
        .await
        .expect("failed to clear quarantine records");
}

#[tokio::test]
async fn test_hash_message() {
    let att = make_attachment(1, "file.png", 10);
    let msg1 = make_message(1, 1, 1, 1, "hello", vec![att.clone()]);
    let msg2 = make_message(2, 1, 1, 1, "hello", vec![att.clone()]);
    let hash1 = hash_message(&msg1);
    let hash2 = hash_message(&msg2);
    assert_eq!(hash1, hash2);

    let msg3 = make_message(3, 1, 1, 1, "world", vec![att.clone()]);
    assert_ne!(hash1, hash_message(&msg3));

    let att2 = make_attachment(2, "other.png", 5);
    let msg4 = make_message(4, 1, 1, 1, "hello", vec![att, att2]);
    assert_ne!(hash1, hash_message(&msg4));
}

#[tokio::test]
async fn test_hash_message_ignores_attachment_filenames_and_order() {
    let first = make_image_attachment(1, "a.png", 10, 100, 200);
    let second = make_image_attachment(2, "b.png", 20, 300, 400);

    let original = make_message(
        1,
        1,
        1,
        1,
        "",
        vec![first.clone(), second.clone()],
    );

    let renamed_first = make_image_attachment(3, "renamed-1.png", 10, 100, 200);
    let renamed_second = make_image_attachment(4, "renamed-2.png", 20, 300, 400);
    let reordered = make_message(
        2,
        2,
        1,
        1,
        "",
        vec![renamed_second, renamed_first],
    );

    assert_eq!(hash_message(&original), hash_message(&reordered));
}

#[tokio::test]
async fn test_log_message_quarantines_multi_attachment_spam_with_renamed_files() {
    let ctx = build_context().await;
    reset_spam_state(&ctx, 1, 9).await;

    for index in 1..SPAM_LIMIT as u64 {
        let message = make_message(
            500 + index,
            index,
            1,
            9,
            "",
            vec![
                make_image_attachment(
                    index * 10 + 1,
                    &format!("img-{index}-1.png"),
                    10,
                    100,
                    200,
                ),
                make_image_attachment(
                    index * 10 + 2,
                    &format!("img-{index}-2.png"),
                    20,
                    300,
                    400,
                ),
                make_image_attachment(
                    index * 10 + 3,
                    &format!("img-{index}-3.png"),
                    30,
                    500,
                    600,
                ),
            ],
        );

        assert!(matches!(
            log_message(&ctx, 1, &message).await,
            LogOutcome::None
        ));
    }

    let final_message = make_message(
        599,
        SPAM_LIMIT as u64,
        1,
        9,
        "",
        vec![
            make_image_attachment(91, "final-a.png", 10, 100, 200),
            make_image_attachment(92, "final-b.png", 20, 300, 400),
            make_image_attachment(93, "final-c.png", 30, 500, 600),
        ],
    );

    assert!(matches!(
        log_message(&ctx, 1, &final_message).await,
        LogOutcome::NewlyQuarantined(_)
    ));
}

#[tokio::test]
async fn test_log_message_quarantines_campaign_with_different_image_sizes() {
    let ctx = build_context().await;
    reset_spam_state(&ctx, 1, 10).await;

    for index in 1..CAMPAIGN_LIMIT as u64 {
        let message = make_message(
            600 + index,
            index,
            1,
            10,
            "check this out https://discord.com/invite/abc123",
            vec![
                make_image_attachment(
                    index * 20 + 1,
                    "promo-a.png",
                    10 + index,
                    100 + index,
                    200 + index,
                ),
                make_image_attachment(
                    index * 20 + 2,
                    "promo-b.png",
                    20 + index,
                    300 + index,
                    400 + index,
                ),
                make_image_attachment(
                    index * 20 + 3,
                    "promo-c.png",
                    30 + index,
                    500 + index,
                    600 + index,
                ),
            ],
        );

        assert!(matches!(
            log_message(&ctx, 1, &message).await,
            LogOutcome::None
        ));
    }

    let trigger = make_message(
        699,
        CAMPAIGN_LIMIT as u64,
        1,
        10,
        "check this out https://discord.com/invite/zzz999",
        vec![
            make_image_attachment(201, "final-1.png", 44, 144, 244),
            make_image_attachment(202, "final-2.png", 54, 344, 444),
            make_image_attachment(203, "final-3.png", 64, 544, 644),
        ],
    );

    assert!(matches!(
        log_message(&ctx, 1, &trigger).await,
        LogOutcome::NewlyQuarantined(_)
    ));
}

#[tokio::test]
async fn test_log_message_campaign_does_not_double_count_same_channel() {
    let ctx = build_context().await;
    reset_spam_state(&ctx, 1, 11).await;

    for message_id in 1..=CAMPAIGN_LIMIT as u64 {
        let message = make_message(
            700 + message_id,
            55,
            1,
            11,
            "visit https://example.com/deal/123456",
            vec![make_image_attachment(message_id, "image.png", 20 + message_id, 100, 200)],
        );

        assert!(matches!(
            log_message(&ctx, 1, &message).await,
            LogOutcome::None
        ));
    }
}

#[tokio::test]
async fn test_log_message_campaign_separates_different_domains() {
    let ctx = build_context().await;
    reset_spam_state(&ctx, 1, 12).await;

    for channel_id in 1..=2_u64 {
        let message = make_message(
            800 + channel_id,
            channel_id,
            1,
            12,
            "check https://discord.com/invite/abc123",
            vec![make_image_attachment(channel_id, "first.png", 20, 100, 200)],
        );
        assert!(matches!(
            log_message(&ctx, 1, &message).await,
            LogOutcome::None
        ));
    }

    for channel_id in 3..=4_u64 {
        let message = make_message(
            800 + channel_id,
            channel_id,
            1,
            12,
            "check https://bad.example/malware/123456",
            vec![make_image_attachment(channel_id, "second.png", 20, 100, 200)],
        );
        assert!(matches!(
            log_message(&ctx, 1, &message).await,
            LogOutcome::None
        ));
    }
}

#[tokio::test]
async fn test_log_message_and_clear() {
    let ctx = build_context().await;
    reset_spam_state(&ctx, 1, 1).await;

    for i in 1..SPAM_LIMIT as u64 {
        let msg = make_message(i, i, 1, 1, "spam", Vec::new());
        assert!(matches!(
            log_message(&ctx, 1, &msg).await,
            LogOutcome::None
        ));
    }
    let msg = make_message(99, SPAM_LIMIT as u64, 1, 1, "spam", Vec::new());
    let token = match log_message(&ctx, 1, &msg).await {
        LogOutcome::NewlyQuarantined(token) => token,
        _ => panic!("expected quarantine trigger"),
    };

    let quarantine_key = "spam:quarantine:1:1";
    let stored: String = redis_get(&ctx.redis, quarantine_key)
        .await
        .unwrap();
    assert_eq!(stored, token);

    let key = "spam:log:1:1";
    let cleared: Option<SpamRecord> = redis_get(&ctx.redis, key).await;
    assert!(cleared.is_none());

    let new_msg = make_message(100, 1, 1, 1, "different", Vec::new());
    log_message(&ctx, 1, &new_msg).await;
    let record: SpamRecord = redis_get(&ctx.redis, key)
        .await
        .unwrap();
    assert_eq!(record.histories.len(), 1);

    clear_log(&ctx.redis, 1, 1).await;
    let none: Option<SpamRecord> = redis_get(&ctx.redis, key).await;
    assert!(none.is_none());
}

#[tokio::test]
async fn test_log_message_concurrent_calls_do_not_lose_updates() {
    let ctx = build_context().await;
    reset_spam_state(&ctx, 1, 20).await;

    let msg_a = make_message(901, 1, 1, 20, "same content", Vec::new());
    let msg_b = make_message(902, 2, 1, 20, "same content", Vec::new());

    let ctx_a = ctx.clone();
    let ctx_b = ctx.clone();
    let (result_a, result_b) = tokio::join!(
        log_message(&ctx_a, 1, &msg_a),
        log_message(&ctx_b, 1, &msg_b),
    );

    assert!(matches!(result_a, LogOutcome::None));
    assert!(matches!(result_b, LogOutcome::None));

    let key = "spam:log:1:20";
    let record: SpamRecord = redis_get(&ctx.redis, key)
        .await
        .expect("expected spam record after concurrent calls");
    assert_eq!(
        record.histories.len(),
        2,
        "both concurrent history entries should be recorded, not just the last writer's"
    );
}

#[tokio::test]
async fn test_clear_log_waits_for_user_lock_before_deleting() {
    let ctx = build_context().await;
    reset_spam_state(&ctx, 1, 21).await;

    let msg = make_message(903, 1, 1, 21, "same content", Vec::new());
    assert!(matches!(
        log_message(&ctx, 1, &msg).await,
        LogOutcome::None
    ));

    let key = "spam:log:1:21";
    let record: Option<SpamRecord> = redis_get(&ctx.redis, key).await;
    assert!(record.is_some());

    let guard = lock_shard(1, 21).lock().await;
    let pool = ctx.redis.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let mut clear_task = tokio::spawn(async move {
        let _ = started_tx.send(());
        clear_log(&pool, 1, 21).await;
    });

    started_rx
        .await
        .expect("clear_log task should start");
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            &mut clear_task
        )
        .await
        .is_err(),
        "clear_log should wait while the per-user log lock is held"
    );

    let record: Option<SpamRecord> = redis_get(&ctx.redis, key).await;
    assert!(
        record.is_some(),
        "clear_log should not delete the spam record until it owns the lock"
    );

    drop(guard);
    clear_task
        .await
        .expect("clear_log task should finish after lock release");
    let record: Option<SpamRecord> = redis_get(&ctx.redis, key).await;
    assert!(record.is_none());
}

#[tokio::test]
async fn test_log_message_is_idempotent_after_quarantine_claim() {
    let ctx = build_context().await;
    reset_spam_state(&ctx, 1, 2).await;

    for i in 1..SPAM_LIMIT as u64 {
        let msg = make_message(i + 100, i + 100, 1, 2, "spam", Vec::new());
        assert!(matches!(
            log_message(&ctx, 1, &msg).await,
            LogOutcome::None
        ));
    }

    let msg = make_message(
        199,
        SPAM_LIMIT as u64 + 100,
        1,
        2,
        "spam",
        Vec::new(),
    );
    let first_token = match log_message(&ctx, 1, &msg).await {
        LogOutcome::NewlyQuarantined(token) => token,
        _ => panic!("expected first quarantine trigger"),
    };

    for i in 1..SPAM_LIMIT as u64 {
        let msg = make_message(i + 200, i + 200, 1, 2, "spam", Vec::new());
        assert!(matches!(
            log_message(&ctx, 1, &msg).await,
            LogOutcome::None
        ));
    }

    let msg = make_message(
        299,
        SPAM_LIMIT as u64 + 200,
        1,
        2,
        "spam",
        Vec::new(),
    );
    assert!(matches!(
        log_message(&ctx, 1, &msg).await,
        LogOutcome::AlreadyQuarantined
    ));

    let stored: String = redis_get(&ctx.redis, "spam:quarantine:1:2")
        .await
        .unwrap();
    assert_eq!(stored, first_token);
}
