use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::Context as _;
use futures::StreamExt as _;
use tokio::sync::mpsc;
use twilight_model::{
    channel::{Attachment, Message},
    guild::Permissions,
    id::{
        Id,
        marker::{GuildMarker, UserMarker},
    },
};

use super::{
    excerpt,
    quarantine::{self, Evidence},
    scoring::{self, Action},
};
use crate::{
    app::App,
    config::ScamConfig,
    db::quarantine::Reason,
    discord::errors,
    features::relay,
    image_worker::{ImageWorker, WorkerError},
};

struct Job {
    app: Arc<App>,
    message: Message,
    enqueued: Instant,
}

/// So one member can't fill the shared queue; the rest wait in their own line.
const MAX_QUEUED_PER_MEMBER: usize = 3;
/// Past this the member floods faster than scans run, so the message is deleted.
const MAX_WAITING_PER_MEMBER: usize = 20;
/// All lines together. Past it the longest line's newest message is deleted like a flood.
const MAX_WAITING: usize = 128;

type Member = (Id<GuildMarker>, Id<UserMarker>);

pub(super) const STAFF: Permissions = Permissions::ADMINISTRATOR
    .union(Permissions::MANAGE_MESSAGES)
    .union(Permissions::MODERATE_MEMBERS);

#[derive(Default)]
struct Pending {
    queued: usize,
    waiting: VecDeque<Job>,
}

#[derive(Default)]
struct Lines {
    members: HashMap<Member, Pending>,
    waiting: usize,
}

pub struct ScamQueue {
    tx: Option<mpsc::Sender<Job>>,
    rx: Mutex<Option<mpsc::Receiver<Job>>>,
    config: ScamConfig,
    pending: Mutex<Lines>,
}

impl ScamQueue {
    pub fn new(config: &ScamConfig) -> anyhow::Result<Self> {
        if !config.enabled {
            return Ok(Self {
                tx: None,
                rx: Mutex::new(None),
                config: config.clone(),
                pending: Mutex::default(),
            });
        }
        let (tx, rx) = mpsc::channel(config.queue_capacity);
        Ok(Self {
            tx: Some(tx),
            rx: Mutex::new(Some(rx)),
            config: config.clone(),
            pending: Mutex::default(),
        })
    }

    pub fn enabled(&self) -> bool {
        self.tx.is_some()
    }

    pub fn eligible<'a>(&self, message: &'a Message) -> Vec<&'a Attachment> {
        eligible(
            &self.config,
            crate::discord::shown_attachments(message),
        )
    }

    pub fn should_scan(&self, app: &App, guild: Id<GuildMarker>, message: &Message) -> bool {
        self.enabled() && app.guild(guild).scam_detect && !self.eligible(message).is_empty()
    }

    pub fn enqueue(&self, app: Arc<App>, message: Message) {
        if self.tx.is_none() {
            return;
        }
        let Some(guild) = message.guild_id else {
            return;
        };
        let key = (guild, message.author.id);
        let job = Job { app, message, enqueued: Instant::now() };
        let (job, flooded) = {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let lines = &mut *pending;
            let entry = lines.members.entry(key).or_default();
            if entry.queued < MAX_QUEUED_PER_MEMBER {
                entry.queued += 1;
                (Some(job), None)
            } else if entry.waiting.len() >= MAX_WAITING_PER_MEMBER {
                (None, Some(job))
            } else if lines.waiting < MAX_WAITING {
                entry.waiting.push_back(job);
                lines.waiting += 1;
                (None, None)
            } else {
                // The longest line pays (oldest first among equals), so alts filling the lines
                // can't get other members' messages deleted.
                let own = entry.waiting.len();
                let evicted = lines
                    .members
                    .values_mut()
                    .max_by_key(|p| {
                        (
                            p.waiting.len(),
                            p.waiting
                                .front()
                                .map(|job| std::cmp::Reverse(job.enqueued)),
                        )
                    })
                    .filter(|p| p.waiting.len() > own)
                    .and_then(|p| p.waiting.pop_back());
                match evicted {
                    Some(evicted) => {
                        lines
                            .members
                            .entry(key)
                            .or_default()
                            .waiting
                            .push_back(job);
                        (None, Some(evicted))
                    }
                    None => (None, Some(job)),
                }
            }
        };
        if let Some(flooded) = flooded {
            metrics::counter!("scam_scans_total", "result" => "flood").increment(1);
            let deleting = delete_later(&flooded.app, &flooded.message, "scam_flood");
            tracing::warn!(
                guild = flooded
                    .message
                    .guild_id
                    .map_or(0, Id::get),
                user = flooded.message.author.id.get(),
                deleting,
                "too many images waiting for a scan"
            );
        }
        if let Some(job) = job {
            self.send(key, job);
        }
    }

    fn send(&self, key: Member, mut job: Job) {
        let Some(tx) = &self.tx else {
            return;
        };
        loop {
            let job_back = match tx.try_send(job) {
                Ok(()) => return,
                Err(
                    mpsc::error::TrySendError::Full(job) | mpsc::error::TrySendError::Closed(job),
                ) => job,
            };
            metrics::counter!("scam_scans_total", "result" => "dropped").increment(1);
            tracing::warn!("scam scan queue full; relaying without a scan");
            relay_later(&job_back.app, job_back.message);
            match self.finish(key) {
                Some(next) => job = next,
                None => return,
            }
        }
    }

    fn finish(&self, key: Member) -> Option<Job> {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = pending.members.get_mut(&key)?;
        if let Some(next) = entry.waiting.pop_front() {
            pending.waiting -= 1;
            return Some(next);
        }
        entry.queued = entry.queued.saturating_sub(1);
        if entry.queued == 0 {
            pending.members.remove(&key);
        }
        None
    }

    fn done(&self, key: Member) {
        if let Some(next) = self.finish(key) {
            self.send(key, next);
        }
    }
}

fn delete_later(app: &Arc<App>, message: &Message, kind: &'static str) -> bool {
    let (app2, channel, id) = (Arc::clone(app), message.channel_id, message.id);
    app.spawn_cleanup(kind, async move {
        if let Err(error) = app2
            .http
            .delete_message(channel, id)
            .await
            && !errors::is(&error, errors::UNKNOWN_MESSAGE)
        {
            tracing::warn!(%error, "failed to delete image message");
        }
    })
}

async fn has_case(app: &App, message: &Message) -> bool {
    case_exists(app, message.guild_id, message.author.id).await
}

async fn case_exists(app: &App, guild: Option<Id<GuildMarker>>, user: Id<UserMarker>) -> bool {
    let Some(guild) = guild else { return false };
    matches!(
        app.db
            .call(move |c| crate::db::quarantine::get(c, guild, user))
            .await,
        Ok(Some(_))
    )
}

async fn sender_quarantined(app: &Arc<App>, message: &Message) -> bool {
    let quarantined = has_case(app, message).await;
    if quarantined {
        metrics::counter!("scam_scans_total", "result" => "quarantined").increment(1);
        delete_later(app, message, "scam_quarantined");
    }
    quarantined
}

/// Every path that lets a held message out comes here, so a sender quarantined meanwhile is caught.
fn relay_later(app: &Arc<App>, message: Message) {
    let (guild, user, channel, id) = (
        message.guild_id,
        message.author.id,
        message.channel_id,
        message.id,
    );
    let relays = guild.is_some_and(|guild| !relay::relay_kinds(app, guild, channel).is_empty());
    let app2 = Arc::clone(app);
    let relayed = relays
        && app.spawn_relay(async move {
            if has_case(&app2, &message).await {
                metrics::counter!("scam_scans_total", "result" => "quarantined").increment(1);
                if let Err(error) = app2
                    .http
                    .delete_message(message.channel_id, message.id)
                    .await
                    && !errors::is(&error, errors::UNKNOWN_MESSAGE)
                {
                    tracing::warn!(%error, "failed to delete image message");
                }
                return;
            }
            relay::on_message(&app2, message).await;
        });
    // Not relayed, but a sender quarantined meanwhile still loses the message.
    if !relayed {
        let app2 = Arc::clone(app);
        app.spawn_cleanup("scam_quarantined", async move {
            if case_exists(&app2, guild, user).await {
                metrics::counter!("scam_scans_total", "result" => "quarantined").increment(1);
                if let Err(error) = app2
                    .http
                    .delete_message(channel, id)
                    .await
                    && !errors::is(&error, errors::UNKNOWN_MESSAGE)
                {
                    tracing::warn!(%error, "failed to delete image message");
                }
            }
        });
    }
}

pub fn eligible<'a>(config: &ScamConfig, attachments: &'a [Attachment]) -> Vec<&'a Attachment> {
    attachments
        .iter()
        .filter(|a| {
            a.content_type
                .as_deref()
                .is_some_and(|t| {
                    matches!(
                        t,
                        "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "image/bmp"
                    )
                })
                && a.size <= config.max_upload_bytes
        })
        .take(config.max_images_per_message)
        .collect()
}

pub fn start(app: &Arc<App>) {
    let Some(mut rx) = app
        .moderation
        .scam
        .rx
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    else {
        return;
    };
    let app2 = Arc::clone(app);
    app.spawn(async move {
        // Not fatal: each scan retries in a fresh worker, and failures are counted.
        if let Err(error) = app2.images.load().await {
            tracing::error!(%error, "OCR engine failed to load; each scan will retry");
        }
        let engine = Some(&app2.images);
        loop {
            let job = tokio::select! {
                () = app2.shutdown.cancelled() => break,
                job = rx.recv() => job,
            };
            let Some(job) = job else { break };
            let key = job
                .message
                .guild_id
                .map(|guild| (guild, job.message.author.id));
            // This task owns the queue; if it died, every later image would go out unscanned.
            if futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(process(job, engine)))
                .await
                .is_err()
                && let Some(key) = key
            {
                app2.moderation.scam.done(key);
            }
        }
    });
}

async fn process(job: Job, engine: Option<&ImageWorker>) {
    let Job { app, message, enqueued } = job;
    let Some(guild) = message.guild_id else { return };
    let key = (guild, message.author.id);
    scan_message(&app, guild, message, enqueued, engine).await;
    app.moderation.scam.done(key);
}

async fn scan_message(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    message: Message,
    enqueued: Instant,
    engine: Option<&ImageWorker>,
) {
    let config = &app.config.scam;
    if sender_quarantined(app, &message).await {
        return;
    }
    let Some(engine) = engine else {
        metrics::counter!("scam_scans_total", "result" => "skipped").increment(1);
        relay_later(app, message);
        return;
    };
    // Past the hold the copy goes out unscanned, but a late block still deletes and quarantines.
    let mut relayed = false;
    if enqueued.elapsed() >= config.job_ttl {
        metrics::counter!("scam_scans_total", "result" => "late").increment(1);
        relay_later(app, message.clone());
        relayed = true;
    }

    for attachment in eligible(
        config,
        crate::discord::shown_attachments(&message),
    ) {
        let started = Instant::now();
        let scan = tokio::time::timeout(scan_limit(config), scan(app, engine, attachment)).await;
        metrics::histogram!("scam_scan_seconds").record(started.elapsed().as_secs_f64());
        let (text, score) = match scan {
            Ok(Ok((text, score, partial))) => {
                // Scored on what was read; a steady count means OCR is too slow for the timeout.
                if partial {
                    metrics::counter!("scam_scans_partial_total").increment(1);
                    tracing::warn!(attachment = %attachment.id, "scam scan stopped at its deadline");
                }
                (text, score)
            }
            // Unreadable even as Discord's copy: relay unscanned.
            Ok(Err(error)) => {
                let result = match error.downcast_ref::<WorkerError>() {
                    Some(WorkerError::Crashed(_)) => "crashed",
                    Some(WorkerError::Late) => "timeout",
                    _ => "error",
                };
                metrics::counter!("scam_scans_total", "result" => result).increment(1);
                tracing::warn!(attachment = %attachment.id, user = message.author.id.get(), %error, "scam scan failed");
                continue;
            }
            Err(_) => {
                metrics::counter!("scam_scans_total", "result" => "timeout").increment(1);
                tracing::warn!(attachment = %attachment.id, "scam scan timed out");
                continue;
            }
        };
        let label = match score.action {
            Action::Allow => "allow",
            Action::Review => "review",
            Action::Block => "block",
        };
        metrics::counter!("scam_scans_total", "result" => label).increment(1);
        if score.action == Action::Block {
            tracing::warn!(
                guild = guild.get(),
                user = message.author.id.get(),
                risk = score.risk,
                reasons = ?score.reasons,
                "scam image detected"
            );
            block(app, guild, &message, &text, &score).await;
            return;
        }
    }
    if !relayed {
        relay_later(app, message);
    }
}

/// A backstop only; each step has its own limit.
fn scan_limit(config: &ScamConfig) -> std::time::Duration {
    config.scan_timeout * 2 + config.download_timeout * 2
}

async fn scan(
    app: &App,
    engine: &ImageWorker,
    attachment: &Attachment,
) -> anyhow::Result<(String, scoring::Score, bool)> {
    let config = &app.config.scam;
    // Counts only while the worker is this scan's; both reads share it.
    let mut budget = config.scan_timeout * 4 / 5;
    let bytes = download(app, &attachment.url, attachment.size, config).await?;
    let (text, partial) = match engine.ocr(&bytes, &mut budget).await {
        Ok(read) => read,
        // Discord's media proxy transcodes files our decoders refuse (e.g. arithmetic-coded
        // JPEG), so scan the copy members see.
        Err(error @ (WorkerError::Refused(_) | WorkerError::Crashed(_))) => {
            drop(bytes);
            if budget.is_zero() {
                return Err(WorkerError::Late.into());
            }
            tracing::debug!(%error, attachment = %attachment.id, "scanning Discord's copy instead");
            let mut url = reqwest::Url::parse(&attachment.proxy_url)?;
            url.query_pairs_mut()
                .append_pair("format", "webp");
            let shown = download(app, url.as_str(), 0, config)
                .await
                .with_context(|| format!("{error}; Discord's copy"))?;
            engine.ocr(&shown, &mut budget).await?
        }
        Err(error) => return Err(error.into()),
    };
    let score = scoring::score_text(
        &text,
        config.block_threshold,
        config.review_threshold,
    );
    Ok((text, score, partial))
}

async fn download(
    app: &App,
    url: &str,
    size_hint: u64,
    config: &ScamConfig,
) -> anyhow::Result<Vec<u8>> {
    let response = app
        .web
        .get(url)
        .timeout(config.download_timeout)
        .send()
        .await
        .context("image download failed")?
        .error_for_status()?;
    let mut body = Vec::with_capacity(size_hint.min(config.max_upload_bytes) as usize);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if (body.len() + chunk.len()) as u64 > config.max_upload_bytes {
            anyhow::bail!("image exceeds size limit");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn block(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    message: &Message,
    text: &str,
    score: &scoring::Score,
) {
    // Before the delete: its MessageDelete would otherwise take the copies at normal priority.
    relay::delete_copies(app, &[message.id], true);
    if let Err(error) = app
        .http
        .delete_message(message.channel_id, message.id)
        .await
        && !errors::is(&error, errors::UNKNOWN_MESSAGE)
    {
        tracing::warn!(%error, "failed to delete scam message");
    }
    // Staff post scam images to report them, so deleting is enough.
    let roles = message
        .member
        .as_ref()
        .map_or(&[][..], |m| m.roles.as_slice());
    if super::holds_any(app, guild, message.author.id, roles, STAFF) {
        metrics::counter!("scam_scans_total", "result" => "staff").increment(1);
        tracing::warn!(
            guild = guild.get(),
            user = message.author.id.get(),
            "scam image from staff deleted without a quarantine"
        );
        return;
    }
    let mut evidence_text = excerpt(message);
    let ocr = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let ocr = crate::discord::ui::truncate(&ocr, 300);
    if !ocr.is_empty() {
        evidence_text.push_str(&format!("\nOCR: {ocr}"));
    }
    let evidence = Evidence {
        summary: format!(
            "risk {:.2} · {}",
            score.risk,
            score.reasons.join(", ")
        ),
        excerpt: evidence_text,
        channel: Some(message.channel_id),
    };
    match quarantine::quarantine(
        app,
        guild,
        &message.author,
        Reason::Scam,
        evidence,
        None,
    )
    .await
    {
        Ok(quarantine::QuarantineOutcome::NotConfigured) => {
            tracing::warn!(
                guild = guild.get(),
                "scam sender not quarantined: quarantine is not set up"
            );
        }
        Ok(_) => {}
        Err(error) => tracing::error!(%error, "failed to quarantine scam sender"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn all_lines_together_are_capped() {
        let app = crate::app::test_app().await;
        let queue = ScamQueue::new(&ScamConfig {
            enabled: true,
            queue_capacity: 1000,
            ..app.config.scam.clone()
        })
        .unwrap();
        let message = |id: u64, author: u64| -> Message {
            serde_json::from_value(serde_json::json!({
                "id": id.to_string(), "channel_id": "20", "guild_id": "30",
                "author": {"id": author.to_string(), "username": "a", "discriminator": "0", "avatar": null},
                "content": "", "timestamp": "2026-11-08T00:00:00+00:00", "edited_timestamp": null,
                "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
                "attachments": [], "embeds": [], "pinned": false, "type": 0
            }))
            .unwrap()
        };
        let mut id = 1;
        for author in 100..114 {
            for _ in 0..MAX_QUEUED_PER_MEMBER + MAX_WAITING_PER_MEMBER {
                queue.enqueue(Arc::clone(&app), message(id, author));
                id += 1;
            }
        }
        let lines = queue.pending.lock().unwrap();
        assert_eq!(lines.waiting, MAX_WAITING);
        let held: usize = lines
            .members
            .values()
            .map(|p| p.waiting.len())
            .sum();
        assert_eq!(held, MAX_WAITING);
        // Later alts took places from the longest lines instead of being deleted outright.
        let longest = lines
            .members
            .values()
            .map(|p| p.waiting.len())
            .max()
            .unwrap();
        assert!(longest < MAX_WAITING_PER_MEMBER);
        drop(lines);
        // A 4th message while full waits; a longest line's newest is deleted instead.
        let victim = (Id::new(30), Id::new(200));
        for _ in 0..=MAX_QUEUED_PER_MEMBER {
            queue.enqueue(Arc::clone(&app), message(id, 200));
            id += 1;
        }
        let lines = queue.pending.lock().unwrap();
        assert_eq!(lines.waiting, MAX_WAITING);
        let waiting = &lines.members[&victim].waiting;
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].message.id, Id::new(id - 1));
        let held: usize = lines
            .members
            .values()
            .map(|p| p.waiting.len())
            .sum();
        assert_eq!(held, MAX_WAITING);
        // Each alt kept its oldest waiting messages (the 4th it posted onwards), in order.
        let per_alt = (MAX_QUEUED_PER_MEMBER + MAX_WAITING_PER_MEMBER) as u64;
        for author in 100..114u64 {
            let line = &lines.members[&(Id::new(30), Id::new(author))].waiting;
            let first = (author - 100) * per_alt + 1 + MAX_QUEUED_PER_MEMBER as u64;
            for (n, job) in line.iter().enumerate() {
                assert_eq!(job.message.id.get(), first + n as u64);
            }
        }
    }

    #[tokio::test]
    async fn equal_lines_evict_the_one_waiting_longest() {
        let app = crate::app::test_app().await;
        let queue = ScamQueue::new(&ScamConfig {
            enabled: true,
            queue_capacity: 128,
            ..app.config.scam.clone()
        })
        .unwrap();
        let mut rx = queue.rx.lock().unwrap().take().unwrap();
        let message = |id: u64, author: u64| -> Message {
            serde_json::from_value(serde_json::json!({
                "id": id.to_string(), "channel_id": "20", "guild_id": "30",
                "author": {"id": author.to_string(), "username": "a", "discriminator": "0", "avatar": null},
                "content": "", "timestamp": "2026-11-08T00:00:00+00:00", "edited_timestamp": null,
                "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
                "attachments": [], "embeds": [], "pinned": false, "type": 0
            }))
            .unwrap()
        };
        let mut id = 1;
        // 41 alts with 3 queued and 3 waiting, one with 3 queued and 2 waiting.
        for author in 100..141 {
            for _ in 0..6 {
                queue.enqueue(Arc::clone(&app), message(id, author));
                id += 1;
            }
        }
        for _ in 0..5 {
            queue.enqueue(Arc::clone(&app), message(id, 141));
            id += 1;
        }
        let _scanning = rx.try_recv().unwrap();
        // A member posting now fills the last waiting place with a line as long as the alts'.
        let victim = (Id::new(30), Id::new(200));
        for _ in 0..6 {
            queue.enqueue(Arc::clone(&app), message(id, 200));
            id += 1;
        }
        assert_eq!(queue.pending.lock().unwrap().waiting, MAX_WAITING);
        queue.enqueue(Arc::clone(&app), message(id, 141));
        let lines = queue.pending.lock().unwrap();
        assert_eq!(lines.members[&victim].waiting.len(), 3);
        assert_eq!(
            lines.members[&(Id::new(30), Id::new(100))]
                .waiting
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn one_member_cannot_fill_the_scan_queue() {
        let app = crate::app::test_app().await;
        let queue =
            ScamQueue::new(&ScamConfig { enabled: true, ..app.config.scam.clone() }).unwrap();
        let message = |id: u64, author: u64| -> Message {
            serde_json::from_value(serde_json::json!({
                "id": id.to_string(), "channel_id": "20", "guild_id": "30",
                "author": {"id": author.to_string(), "username": "a", "discriminator": "0", "avatar": null},
                "content": "", "timestamp": "2026-11-08T00:00:00+00:00", "edited_timestamp": null,
                "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
                "attachments": [], "embeds": [], "pinned": false, "type": 0
            }))
            .unwrap()
        };
        for id in 1..=5 {
            queue.enqueue(Arc::clone(&app), message(id, 7));
        }
        queue.enqueue(Arc::clone(&app), message(9, 8));
        let mut rx = queue.rx.lock().unwrap().take().unwrap();
        let mut queued = Vec::new();
        while let Ok(job) = rx.try_recv() {
            queued.push((job.message.author.id.get(), job.message.id.get()));
        }
        assert_eq!(queued, [(7, 1), (7, 2), (7, 3), (8, 9)]);
        // The rest are not dropped: each finished scan lets the member's next message in.
        let member = (Id::new(30), Id::new(7));
        queue.done(member);
        assert_eq!(rx.try_recv().unwrap().message.id.get(), 4);
        queue.done(member);
        assert_eq!(rx.try_recv().unwrap().message.id.get(), 5);
        assert!(rx.try_recv().is_err());
        for _ in 0..3 {
            queue.done(member);
        }
        assert!(
            !queue
                .pending
                .lock()
                .unwrap()
                .members
                .contains_key(&member)
        );
    }
}
