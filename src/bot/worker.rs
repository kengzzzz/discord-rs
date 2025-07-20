use metrics::Histogram;
use once_cell::sync::Lazy;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc::Receiver};
use tokio::task::JoinHandle;
use twilight_gateway::Event;

use super::PriorityClass;
use crate::context::Context;

pub struct EnqueuedEvent {
    pub event: Event,
    pub enqueue_at: std::time::Instant,
}

pub struct Worker;

static HIGH_QUEUE_WAIT: Lazy<Histogram> =
    Lazy::new(|| metrics::histogram!("bot_queue_wait_seconds", "priority" => "high"));
static NORMAL_QUEUE_WAIT: Lazy<Histogram> =
    Lazy::new(|| metrics::histogram!("bot_queue_wait_seconds", "priority" => "normal"));
static LOW_QUEUE_WAIT: Lazy<Histogram> =
    Lazy::new(|| metrics::histogram!("bot_queue_wait_seconds", "priority" => "low"));

impl Worker {
    pub fn spawn<F, Fut>(
        ctx: Arc<Context>,
        sem: Arc<Semaphore>,
        mut rx: Receiver<EnqueuedEvent>,
        priority: PriorityClass,
        handler: F,
    ) -> JoinHandle<()>
    where
        F: Fn(Arc<Context>, Event) -> Fut + Send + Sync + 'static + Copy,
        Fut: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(async move {
            while let Some(EnqueuedEvent { event, enqueue_at }) = rx.recv().await {
                let wait = enqueue_at.elapsed().as_secs_f64();
                match priority {
                    PriorityClass::High => HIGH_QUEUE_WAIT.record(wait),
                    PriorityClass::Normal => NORMAL_QUEUE_WAIT.record(wait),
                    PriorityClass::Low => LOW_QUEUE_WAIT.record(wait),
                    PriorityClass::Ignore => {}
                }

                let permit = match sem.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => {
                        tracing::info!(
                            "{} semaphore closed, exit worker",
                            priority.as_str()
                        );
                        break;
                    }
                };

                let ctx2 = ctx.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    handler(ctx2, event).await;
                });
            }
        })
    }
}
