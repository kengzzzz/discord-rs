use once_cell::sync::OnceCell;
use tokio_util::sync::CancellationToken;

static SHUTDOWN: OnceCell<CancellationToken> = OnceCell::new();

pub fn set_token(token: CancellationToken) {
    let _ = SHUTDOWN.set(token);
}

pub fn get_token() -> CancellationToken {
    SHUTDOWN.get().expect("shutdown token not set").clone()
}
