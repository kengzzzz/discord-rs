use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiOperation {
    Chat,
    Summary,
}

impl AiOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Summary => "summary",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AdmissionConfig {
    pub rpm_limit: usize,
    pub queue_timeout: Duration,
}

#[derive(Clone)]
pub struct AiScheduler {
    inner: Arc<AiSchedulerInner>,
}

struct AiSchedulerInner {
    rpm_window: Duration,
    models: Mutex<HashMap<&'static str, Arc<ModelQueue>>>,
}

struct ModelQueue {
    state: AsyncMutex<ModelState>,
    waiting: AtomicUsize,
}

#[derive(Default)]
struct ModelState {
    recent: VecDeque<Instant>,
    cooldown_until: Option<Instant>,
}

impl Default for AiScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl AiScheduler {
    pub fn new() -> Self {
        Self::with_window(Duration::from_secs(60))
    }

    pub(crate) fn with_window(rpm_window: Duration) -> Self {
        Self {
            inner: Arc::new(AiSchedulerInner { rpm_window, models: Mutex::new(HashMap::new()) }),
        }
    }

    pub async fn acquire(
        &self,
        model: &'static str,
        operation: AiOperation,
        cfg: AdmissionConfig,
    ) -> anyhow::Result<ScheduleGuard> {
        let queue = self.queue(model);
        let enqueued_at = Instant::now();
        let deadline = enqueued_at + cfg.queue_timeout;

        let depth = queue
            .waiting
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        metrics::gauge!("ai_scheduler_queue_depth", "model" => model).set(depth as f64);

        loop {
            let now = Instant::now();
            let mut state = queue.state.lock().await;
            state.prune(self.inner.rpm_window, now);

            if let Some(wait) = state.required_wait(now, cfg.rpm_limit, self.inner.rpm_window) {
                drop(state);

                let remaining = deadline.saturating_duration_since(now);
                if wait >= remaining {
                    let depth = queue
                        .waiting
                        .fetch_sub(1, Ordering::Relaxed)
                        .saturating_sub(1);
                    metrics::gauge!("ai_scheduler_queue_depth", "model" => model).set(depth as f64);
                    metrics::counter!(
                        "ai_scheduler_requests_total",
                        "model" => model,
                        "operation" => operation.as_str(),
                        "result" => "timeout",
                    )
                    .increment(1);
                    return Err(anyhow!(
                        "ai scheduler queue timeout for model {model}"
                    ));
                }

                tokio::time::sleep(wait).await;
                continue;
            }

            state.recent.push_back(now);
            drop(state);

            let depth = queue
                .waiting
                .fetch_sub(1, Ordering::Relaxed)
                .saturating_sub(1);
            metrics::gauge!("ai_scheduler_queue_depth", "model" => model).set(depth as f64);
            metrics::histogram!(
                "ai_scheduler_queue_wait_seconds",
                "model" => model,
                "operation" => operation.as_str(),
            )
            .record(enqueued_at.elapsed().as_secs_f64());
            metrics::counter!(
                "ai_scheduler_requests_total",
                "model" => model,
                "operation" => operation.as_str(),
                "result" => "admitted",
            )
            .increment(1);
            return Ok(ScheduleGuard { scheduler: self.clone(), model, operation });
        }
    }

    pub async fn cool_down(&self, model: &'static str, operation: AiOperation, duration: Duration) {
        let queue = self.queue(model);
        let mut state = queue.state.lock().await;
        let until = Instant::now() + duration;
        state.cooldown_until = Some(match state.cooldown_until {
            Some(current) if current > until => current,
            _ => until,
        });
        metrics::counter!(
            "ai_scheduler_cooldowns_total",
            "model" => model,
            "operation" => operation.as_str(),
        )
        .increment(1);
    }

    fn queue(&self, model: &'static str) -> Arc<ModelQueue> {
        let mut models = self
            .inner
            .models
            .lock()
            .expect("ai scheduler model map poisoned");
        models
            .entry(model)
            .or_insert_with(|| {
                Arc::new(ModelQueue {
                    state: AsyncMutex::new(ModelState::default()),
                    waiting: AtomicUsize::new(0),
                })
            })
            .clone()
    }
}

pub struct ScheduleGuard {
    scheduler: AiScheduler,
    model: &'static str,
    operation: AiOperation,
}

impl ScheduleGuard {
    pub async fn cool_down(self, duration: Duration) {
        self.scheduler
            .cool_down(self.model, self.operation, duration)
            .await;
    }
}

impl ModelState {
    fn prune(&mut self, rpm_window: Duration, now: Instant) {
        while self
            .recent
            .front()
            .is_some_and(|seen| now.duration_since(*seen) >= rpm_window)
        {
            self.recent.pop_front();
        }

        if self
            .cooldown_until
            .is_some_and(|until| until <= now)
        {
            self.cooldown_until = None;
        }
    }

    fn required_wait(
        &self,
        now: Instant,
        rpm_limit: usize,
        rpm_window: Duration,
    ) -> Option<Duration> {
        let cooldown_wait = self
            .cooldown_until
            .and_then(|until| until.checked_duration_since(now));

        let rpm_wait = if rpm_limit > 0 && self.recent.len() >= rpm_limit {
            self.recent
                .front()
                .and_then(|seen| (*seen + rpm_window).checked_duration_since(now))
        } else {
            None
        };

        match (cooldown_wait, rpm_wait) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }
}

#[cfg(all(test, not(feature = "test-utils")))]
#[path = "tests/scheduler.rs"]
mod tests;
