use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    },
};

use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tokio_util::sync::CancellationToken;

use crate::db::Db;

#[derive(Default)]
pub struct Health {
    ready: AtomicBool,
    gateway: AtomicBool,
    latency_ms: AtomicU64,
    last_event: AtomicI64,
}

impl Health {
    pub fn set_ready(&self, ready: bool) {
        self.ready
            .store(ready, Ordering::Relaxed);
    }

    pub fn set_gateway(&self, connected: bool, latency_ms: Option<u64>) {
        self.gateway
            .store(connected, Ordering::Relaxed);
        if let Some(ms) = latency_ms {
            self.latency_ms
                .store(ms, Ordering::Relaxed);
        }
        self.last_event
            .store(crate::db::now(), Ordering::Relaxed);
    }

    pub fn latency_ms(&self) -> Option<u64> {
        let ms = self.latency_ms.load(Ordering::Relaxed);
        (ms > 0).then_some(ms)
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed) && self.gateway.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
struct ServerState {
    health: Arc<HealthView>,
    metrics: PrometheusHandle,
}

pub struct HealthView {
    pub health: Arc<dyn Fn() -> bool + Send + Sync>,
    pub db: Db,
}

pub fn install_metrics() -> anyhow::Result<PrometheusHandle> {
    Ok(PrometheusBuilder::new().install_recorder()?)
}

pub async fn serve(
    addr: SocketAddr,
    view: HealthView,
    metrics: PrometheusHandle,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    // Histogram samples pile up if nothing scrapes /metrics.
    let upkeep = metrics.clone();
    let upkeep_stop = shutdown.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tokio::select! {
                () = upkeep_stop.cancelled() => break,
                _ = tick.tick() => upkeep.run_upkeep(),
            }
        }
    });
    let state = ServerState { health: Arc::new(view), metrics };
    let router = Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(render_metrics))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async move { shutdown.cancelled().await })
        .await?;
    Ok(())
}

async fn healthz(State(state): State<ServerState>) -> impl IntoResponse {
    let db_ok = state
        .health
        .db
        .call(|c| c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0)))
        .await
        .is_ok();
    if (state.health.health)() && db_ok {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "unhealthy")
    }
}

async fn render_metrics(State(state): State<ServerState>) -> impl IntoResponse {
    state.metrics.render()
}
