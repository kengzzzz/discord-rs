use std::future::Future;
use tokio::task::JoinHandle;

use axum::{Router, routing::get};

use crate::{observability::metrics::init_metrics, services::health::HealthService};

pub struct ServerConfig<F> {
    pub shutdown: F,
}

pub fn start_server<F>(config: ServerConfig<F>) -> JoinHandle<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let metrics_handle = init_metrics().expect("initialize prometheus failed");
        let app = Router::new()
            .route("/healthz", get(HealthService::health))
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
            .with_graceful_shutdown(config.shutdown)
            .await
            .expect("health server crashed");
    })
}
