pub mod builds;
pub mod circuit;
pub mod cycles;
pub mod market;
pub mod notify;
pub mod status;
pub mod worldstate;

use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

use crate::app::App;

use self::worldstate::Snapshot;

const POLL: Duration = Duration::from_secs(60);
const MARKET_REFRESH: Duration = Duration::from_secs(60 * 60);
const BUILDS_REFRESH: Duration = Duration::from_secs(12 * 60 * 60);
const CATALOG_RETRY: Duration = Duration::from_secs(5 * 60);
const NOTIFY_LOG_KEPT_SECS: i64 = 30 * 24 * 60 * 60;
const NOTIFY_LOG_PRUNE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Default)]
pub struct Warframe {
    snapshot: RwLock<Option<Arc<Snapshot>>>,
    /// One upstream body at a time; a popular item's orders are about 1 MB.
    lookup: tokio::sync::Mutex<()>,
    fetched_at: AtomicI64,
    bounty_end: AtomicI64,
    pub market: market::Market,
    pub builds: builds::Builds,
}

impl Warframe {
    pub fn snapshot(&self) -> Option<Arc<Snapshot>> {
        self.snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn fetched_at(&self) -> Option<i64> {
        let at = self.fetched_at.load(Ordering::Relaxed);
        (at > 0).then_some(at)
    }

    pub fn last_bounty_end(&self) -> Option<i64> {
        let at = self.bounty_end.load(Ordering::Relaxed);
        (at > 0).then_some(at)
    }

    fn store(&self, snapshot: Snapshot, now: i64) {
        if let Some(bounty) = snapshot.bounty_end {
            self.bounty_end
                .store(bounty, Ordering::Relaxed);
        }
        self.fetched_at
            .store(now, Ordering::Relaxed);
        *self
            .snapshot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(snapshot));
    }
}

const LOOKUP_WAIT: Duration = Duration::from_secs(5);
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
pub const LOOKUP_BODY_LIMIT: usize = 2 << 20;

pub async fn lookup(app: &App) -> anyhow::Result<tokio::sync::MutexGuard<'_, ()>> {
    tokio::time::timeout(LOOKUP_WAIT, app.warframe.lookup.lock())
        .await
        .map_err(|_| Busy.into())
}

#[derive(Debug)]
struct Busy;

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("upstream lookups busy")
    }
}

impl std::error::Error for Busy {}

pub fn log_failure(upstream: &'static str, error: &anyhow::Error) {
    if error.is::<Busy>() {
        metrics::counter!("upstream_busy_total", "upstream" => upstream).increment(1);
    } else {
        tracing::warn!(upstream, %error, "upstream request failed");
    }
}

pub async fn read_limited(request: reqwest::RequestBuilder) -> anyhow::Result<Vec<u8>> {
    let response = request
        .timeout(LOOKUP_TIMEOUT)
        .send()
        .await?
        .error_for_status()?;
    read_capped(response, LOOKUP_BODY_LIMIT).await
}

pub async fn read_capped(mut response: reqwest::Response, limit: usize) -> anyhow::Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > limit {
            anyhow::bail!("upstream body over {limit} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn fetch(app: &App) -> anyhow::Result<Snapshot> {
    let body = app
        .web
        .get(worldstate::URL)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    worldstate::parse(&body, jiff::Timestamp::now().as_millisecond())
}

pub fn start(app: &Arc<App>) {
    let poller = Arc::clone(app);
    app.spawn(async move {
        let mut tick = tokio::time::interval(POLL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut fetch_failing = false;
        let mut status_failing = false;
        let mut pruned_at: Option<tokio::time::Instant> = None;
        let mut checked = std::collections::HashSet::new();
        loop {
            tokio::select! {
                () = poller.shutdown.cancelled() => break,
                _ = tick.tick() => {}
            }
            let fetched = tokio::select! {
                () = poller.shutdown.cancelled() => break,
                fetched = fetch(&poller) => fetched,
            };
            match fetched {
                Ok(snapshot) => {
                    if fetch_failing {
                        tracing::info!("worldstate fetch recovered");
                        fetch_failing = false;
                    }
                    poller.warframe.store(snapshot, jiff::Timestamp::now().as_millisecond());
                }
                Err(error) => {
                    if !fetch_failing {
                        tracing::warn!(%error, "worldstate fetch failing; using last snapshot and formulas");
                        fetch_failing = true;
                    }
                }
            }
            if poller.shutdown.is_cancelled() {
                break;
            }
            let failed = status::update_all(&poller, &mut checked).await;
            match failed.first() {
                Some((guild, error)) if !status_failing => {
                    tracing::warn!(guild = guild.get(), guilds = failed.len(), %error, "status update failing; will keep retrying");
                    status_failing = true;
                }
                None if status_failing => {
                    tracing::info!("status updates recovered");
                    status_failing = false;
                }
                _ => {}
            }
            if poller.shutdown.is_cancelled() {
                break;
            }
            notify::tick(&poller).await;
            poller.warframe.market.sweep();
            if pruned_at.is_none_or(|at| at.elapsed() >= NOTIFY_LOG_PRUNE) {
                pruned_at = Some(tokio::time::Instant::now());
                let cutoff = crate::db::now() - NOTIFY_LOG_KEPT_SECS;
                if let Err(error) = poller
                    .db
                    .call(move |c| crate::db::notify::prune(c, cutoff))
                    .await
                {
                    tracing::warn!(%error, "failed to prune notification log");
                }
            }
        }
    });

    let catalogs = Arc::clone(app);
    app.spawn(async move {
        market::load(&catalogs).await;
        builds::load(&catalogs).await;
        let now = tokio::time::Instant::now();
        let (mut market_at, mut builds_at) = (now, now);
        loop {
            let market_due = tokio::select! {
                () = catalogs.shutdown.cancelled() => break,
                () = tokio::time::sleep_until(market_at) => true,
                () = tokio::time::sleep_until(builds_at) => false,
            };
            let refreshed = tokio::select! {
                () = catalogs.shutdown.cancelled() => break,
                refreshed = async {
                    if market_due {
                        market::refresh(&catalogs).await
                    } else {
                        builds::refresh(&catalogs).await
                    }
                } => refreshed,
            };
            let next = match refreshed {
                Ok(()) if market_due => MARKET_REFRESH,
                Ok(()) => BUILDS_REFRESH,
                Err(error) => {
                    let catalog = if market_due { "warframe.market" } else { "build" };
                    tracing::warn!(catalog, %error, "catalog refresh failed");
                    CATALOG_RETRY
                }
            };
            let next = tokio::time::Instant::now() + next;
            if market_due {
                market_at = next;
            } else {
                builds_at = next;
            }
        }
    });
}
