use futures::future;
use mini_redis::server;
use once_cell::sync::OnceCell;
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::{context::Context, services::spam::SpamService};
use twilight_model::{
    channel::{Message, message::MessageType},
    id::{Id, marker::GuildMarker},
    user::User,
    util::datetime::Timestamp,
};

static REDIS_PORT: OnceCell<u16> = OnceCell::new();
static REDIS_HANDLE: OnceCell<tokio::task::JoinHandle<()>> = OnceCell::new();

async fn init_redis() -> u16 {
    if let Some(port) = REDIS_PORT.get() {
        return *port;
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    unsafe {
        std::env::set_var("REDIS_URL", format!("redis://127.0.0.1:{port}"));
    }
    let handle = tokio::spawn(async move {
        let _ = server::run(listener, future::pending::<()>()).await;
    });
    let _ = REDIS_HANDLE.set(handle);
    REDIS_PORT.set(port).unwrap();
    // give server time to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    port
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

fn make_message(channel: u64, id: u64, user: u64, content: &str) -> Message {
    Message {
        activity: None,
        application: None,
        application_id: None,
        attachments: Vec::new(),
        author: dummy_user(user),
        call: None,
        channel_id: Id::new(channel),
        components: Vec::new(),
        content: content.into(),
        edited_timestamp: None,
        embeds: Vec::new(),
        flags: None,
        guild_id: Some(Id::<GuildMarker>::new(1)),
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
        timestamp: Timestamp::from_secs(0).unwrap(),
        thread: None,
        tts: false,
        webhook_id: None,
    }
}

#[tokio::test]
async fn test_spam_log_threshold() {
    init_redis().await;
    let ctx = Arc::new(Context::test().await);
    let mut token = None;
    for i in 0..4u64 {
        let msg = make_message(i + 1, i + 10, 5, "hello");
        token = SpamService::log_message(ctx.clone(), 1, &msg).await;
    }
    assert!(token.is_some());
    assert_eq!(token.unwrap().len(), 6);
    SpamService::purge_cache(1, 5).await;
}

#[tokio::test]
async fn test_spam_log_reset() {
    init_redis().await;
    let ctx = Arc::new(Context::test().await);
    let msg1 = make_message(1, 100, 6, "hi");
    assert!(
        SpamService::log_message(ctx.clone(), 1, &msg1)
            .await
            .is_none()
    );
    let msg2 = make_message(2, 101, 6, "hi");
    assert!(
        SpamService::log_message(ctx.clone(), 1, &msg2)
            .await
            .is_none()
    );
    let msg3 = make_message(3, 102, 6, "bye");
    assert!(
        SpamService::log_message(ctx.clone(), 1, &msg3)
            .await
            .is_none()
    );
    let msg4 = make_message(4, 103, 6, "hi");
    assert!(
        SpamService::log_message(ctx.clone(), 1, &msg4)
            .await
            .is_none()
    );
    // reset because content changed before reaching limit
    let msg5 = make_message(5, 104, 6, "hi");
    let tok = SpamService::log_message(ctx.clone(), 1, &msg5).await;
    assert!(tok.is_none());
    SpamService::purge_cache(1, 6).await;
}
