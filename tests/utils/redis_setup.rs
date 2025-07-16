use once_cell::sync::OnceCell;

static REDIS_STARTED: OnceCell<()> = OnceCell::new();

pub async fn start() {
    if REDIS_STARTED.get().is_some() {
        return;
    }
    unsafe {
        std::env::set_var("REDIS_URL", "redis://127.0.0.1:6379");
    }
    let _ = REDIS_STARTED.set(());
}

#[allow(dead_code)]
pub fn stop() {}
