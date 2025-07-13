use tokio_util::sync::CancellationToken;

use crate::tests::redis_setup;
use crate::{context::Context, services::shutdown};

#[tokio::test]
async fn test_startup_and_shutdown() {
    redis_setup::start().await;
    let token = CancellationToken::new();
    shutdown::set_token(token.clone());
    let ctx = Context::test().await;

    assert_eq!(ctx.cache.stats().guilds(), 0);

    let handle = tokio::spawn(async {
        shutdown::get_token().cancelled().await;
        1u8
    });
    token.cancel();
    assert_eq!(handle.await.unwrap(), 1u8);
    redis_setup::stop();
}
