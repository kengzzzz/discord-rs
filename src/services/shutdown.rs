use once_cell::sync::OnceCell;
use tokio_util::sync::CancellationToken;

static SHUTDOWN: OnceCell<CancellationToken> = OnceCell::new();

pub fn set_token(token: CancellationToken) {
    if SHUTDOWN.set(token).is_err() {
        tracing::warn!("shutdown token already set");
    }
}

pub fn get_token() -> CancellationToken {
    SHUTDOWN
        .get()
        .expect("shutdown token not set")
        .clone()
}
