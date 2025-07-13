use futures::future;
use mini_redis::server;
use once_cell::sync::OnceCell;
use std::time::Duration;
use tokio::{net::TcpListener, runtime::Runtime, time::sleep};

static RUNTIME: OnceCell<Runtime> = OnceCell::new();
static REDIS_STARTED: OnceCell<()> = OnceCell::new();

pub async fn start() {
    if REDIS_STARTED.get().is_some() {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    unsafe {
        std::env::set_var("REDIS_URL", format!("redis://127.0.0.1:{port}"));
    }
    let rt = RUNTIME.get_or_init(|| Runtime::new().unwrap());
    rt.spawn(async move {
        let _ = server::run(listener, future::pending::<()>()).await;
    });
    sleep(Duration::from_millis(50)).await;
    let _ = REDIS_STARTED.set(());
}

pub fn stop() {}
