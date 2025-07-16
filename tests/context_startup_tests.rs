#![allow(unused_imports)]
#![cfg(feature = "mock-redis")]

use tokio_util::sync::CancellationToken;

mod utils;
use discord_bot::services::shutdown;
use utils::context::test_context;

#[tokio::test]
async fn test_startup_and_shutdown() {
    let token = CancellationToken::new();
    shutdown::set_token(token.clone());
    let ctx = test_context().await;

    assert_eq!(ctx.cache.stats().guilds(), 0);

    let handle = tokio::spawn(async {
        shutdown::get_token().cancelled().await;
        1u8
    });
    token.cancel();
    assert_eq!(handle.await.unwrap(), 1u8);
}
