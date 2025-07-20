use std::future::Future;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::{Router, http::StatusCode, routing::get};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

static READY: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));
static DISCORD_CONNECTED: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));
static MONGO_CONNECTED: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));

const QUEUE_WAIT_BUCKETS: &[f64] = &[
    0.00005, 0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0,
    2.0, 3.0,
];
const ENQUEUE_BLOCK_BUCKETS: &[f64] = &[
    0.00005, 0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0,
    2.0, 3.0,
];

pub fn init_metrics() -> anyhow::Result<PrometheusHandle> {
    let builder = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("bot_queue_wait_seconds".to_string()),
            QUEUE_WAIT_BUCKETS,
        )?
        .set_buckets_for_metric(
            Matcher::Full("bot_queue_enqueue_block_seconds".to_string()),
            ENQUEUE_BLOCK_BUCKETS,
        )?;
    let handle = builder.install_recorder()?;
    Ok(handle)
}

pub struct HealthService;

impl HealthService {
    pub fn spawn(shutdown: impl Future<Output = ()> + Send + 'static) {
        tokio::spawn(async move {
            let metrics_handle = init_metrics().expect("initialize prometheus failed");
            let app = Router::new()
                .route("/healthz", get(Self::health))
                .route(
                    "/metrics",
                    get({
                        let handle = metrics_handle.clone();
                        move || async move { handle.render() }
                    }),
                );
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

    pub async fn health() -> StatusCode {
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
