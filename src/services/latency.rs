use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub struct LatencyService;

static LAST_LATENCY_MS: LazyLock<AtomicU64> = LazyLock::new(|| AtomicU64::new(0));

impl LatencyService {
    pub fn update(latency: Option<Duration>) {
        let ms = latency.map(|d| d.as_millis() as u64).unwrap_or(0);
        LAST_LATENCY_MS.store(ms, Ordering::Relaxed);
    }

    pub fn get() -> Option<u64> {
        let val = LAST_LATENCY_MS.load(Ordering::Relaxed);
        if val == 0 { None } else { Some(val) }
    }
}
