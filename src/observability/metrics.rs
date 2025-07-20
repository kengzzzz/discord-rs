use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

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
