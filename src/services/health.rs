use std::future::Future;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::{Router, http::StatusCode, routing::get};

static READY: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));
static DISCORD_CONNECTED: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));
static MONGO_CONNECTED: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));

pub struct HealthService;

impl HealthService {
    pub fn spawn(shutdown: impl Future<Output = ()> + Send + 'static) {
        tokio::spawn(async move {
            let app = Router::new().route("/healthz", get(Self::health));
            let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
                .await
                .expect("bind health listener");
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown)
                .await
                .expect("health server crashed");
        });
    }

    pub fn set_ready(state: bool) {
        READY.store(state, Ordering::Relaxed);
    }

    pub fn set_discord(state: bool) {
        DISCORD_CONNECTED.store(state, Ordering::Relaxed);
    }

    pub fn set_mongo(state: bool) {
        MONGO_CONNECTED.store(state, Ordering::Relaxed);
    }

    async fn health() -> StatusCode {
        if READY.load(Ordering::Relaxed)
            && DISCORD_CONNECTED.load(Ordering::Relaxed)
            && MONGO_CONNECTED.load(Ordering::Relaxed)
        {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}
