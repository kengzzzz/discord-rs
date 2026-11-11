pub mod gemini;
pub mod prompts;
pub mod search;

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use twilight_model::{
    application::interaction::Interaction,
    channel::{
        Attachment, Message,
        message::{Component, MessageFlags, component::ButtonStyle},
    },
    guild::Permissions,
    id::{
        Id,
        marker::{ChannelMarker, UserMarker},
    },
};

use crate::{
    app::{self, App},
    config::AiConfig,
    db::{self, ai::Scope},
    discord::{
        reply,
        ui::{self, Card, color},
    },
};

use self::gemini::{AiError, Effort, Gemini, Part, Reply, Request};

pub const SHARE_PREFIX: &str = "ai:share:";
const SHARE_TTL: Duration = Duration::from_secs(14 * 60);
const SHARE_BYTES: usize = 2 << 20;
const MAX_RUNNING: usize = 4;
/// Longer than any request can run.
const CLEARED_KEPT: Duration = Duration::from_secs(10 * 60);
const MAX_ATTACHMENTS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Translate,
    Chat,
}

struct Share {
    at: Instant,
    owner: Id<UserMarker>,
    channel: Id<ChannelMarker>,
    text: String,
    source: Option<String>,
}

/// 4000 characters of up to 4 bytes, JSON-escaped.
const PAGE_BODY_BYTES: usize = 2 * 4 * 4000;

impl Share {
    /// The most a share holds while posting: its cards plus one page's request body.
    fn bytes(&self) -> usize {
        let text = self.text.len()
            + self
                .source
                .as_ref()
                .map_or(0, String::len);
        let shown = text + text / 2;
        shown + shown.min(PAGE_BODY_BYTES) + 2048
    }
}

#[derive(Default)]
struct Shares {
    saved: HashMap<u64, Share>,
    posting: usize,
}

/// Released on drop, however the post ends.
struct Posting {
    app: Arc<App>,
    bytes: usize,
}

impl Drop for Posting {
    fn drop(&mut self) {
        let mut shares = self
            .app
            .ai
            .shares
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shares.posting = shares
            .posting
            .saturating_sub(self.bytes);
    }
}

pub struct Ai {
    gemini: Gemini,
    cooldowns: Mutex<HashMap<Id<UserMarker>, Instant>>,
    busy: Mutex<HashSet<Id<UserMarker>>>,
    shares: Mutex<Shares>,
    /// Bounds concurrent downloads, up to 15 MB each.
    media: tokio::sync::Semaphore,
    /// A request can hold an event slot for 75 s, so a mention raid can't take them all.
    running: tokio::sync::Semaphore,
    /// So a reply still generating can't write cleared history back.
    cleared: Arc<Mutex<HashMap<Id<UserMarker>, Instant>>>,
    daily: Mutex<HashMap<Id<UserMarker>, Daily>>,
}

#[derive(Debug)]
enum Refused {
    TooSoon,
    Busy,
    /// `first` is true once a day, so the channel notice is sent once.
    DailyLimit {
        first: bool,
    },
}

impl Refused {
    fn card(&self) -> Vec<Component> {
        match self {
            Self::TooSoon => ui::notice(color::WARNING, "รอสักครู่", "คุณส่งคำขอเร็วเกินไป"),
            Self::Busy => ui::notice(
                color::WARNING,
                "รอสักครู่",
                "AI กำลังตอบคำขออื่นอยู่ ลองใหม่อีกครั้ง",
            ),
            Self::DailyLimit { .. } => ui::notice(
                color::WARNING,
                "ครบโควตาวันนี้แล้ว",
                "ใช้ AI ได้อีกครั้งหลัง 07:00 น.",
            ),
        }
    }
}

struct Daily {
    day: i64,
    count: u32,
    notified: bool,
}

struct BusyGuard<'a> {
    ai: &'a Ai,
    user: Id<UserMarker>,
    admitted: Instant,
    _slot: tokio::sync::SemaphorePermit<'a>,
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.ai
            .busy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.user);
    }
}

impl Ai {
    pub fn new(http: reqwest::Client, config: &AiConfig) -> Self {
        Self {
            gemini: Gemini::new(http, config.api_key.clone()),
            cooldowns: Mutex::new(HashMap::new()),
            busy: Mutex::new(HashSet::new()),
            shares: Mutex::new(Shares::default()),
            media: tokio::sync::Semaphore::new(2),
            running: tokio::sync::Semaphore::new(MAX_RUNNING),
            cleared: Arc::new(Mutex::new(HashMap::new())),
            daily: Mutex::new(HashMap::new()),
        }
    }

    pub fn configured(&self) -> bool {
        self.gemini.configured()
    }

    pub async fn generate(
        &self,
        models: &[String],
        request: &Request<'_>,
    ) -> Result<Reply, AiError> {
        self.gemini
            .generate(models, request, request.grounding)
            .await
    }

    fn admit(
        &self,
        user: Id<UserMarker>,
        cooldown: Duration,
        daily_limit: u32,
    ) -> Result<BusyGuard<'_>, Refused> {
        let now = Instant::now();
        {
            let mut cooldowns = self
                .cooldowns
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if cooldowns
                .get(&user)
                .is_some_and(|at| now.duration_since(*at) < cooldown)
            {
                return Err(Refused::TooSoon);
            }
            cooldowns.insert(user, now);
        }
        // The free-tier quota is shared by every guild.
        let today = db::now().div_euclid(86_400);
        {
            let mut daily = self
                .daily
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let entry =
                daily
                    .entry(user)
                    .or_insert(Daily { day: today, count: 0, notified: false });
            if entry.day != today {
                *entry = Daily { day: today, count: 0, notified: false };
            }
            if daily_limit > 0 && entry.count >= daily_limit {
                let first = !entry.notified;
                entry.notified = true;
                return Err(Refused::DailyLimit { first });
            }
        }
        let slot = self
            .running
            .try_acquire()
            .map_err(|_| Refused::Busy)?;
        let mut busy = self
            .busy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // `then`, not `then_some`: an eager guard would drop while `busy` is locked and deadlock.
        let guard = busy
            .insert(user)
            .then(|| BusyGuard { ai: self, user, admitted: now, _slot: slot })
            .ok_or(Refused::Busy)?;
        if let Some(entry) = self
            .daily
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&user)
        {
            entry.count += 1;
        }
        Ok(guard)
    }

    /// Call before queueing the history delete.
    pub fn history_cleared(&self, user: Id<UserMarker>) {
        self.cleared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(user, Instant::now());
    }

    async fn run(&self, app: &App, task: Task, request: &Request<'_>) -> Result<Reply, AiError> {
        let config = &app.config.ai;
        if task == Task::Chat
            && request.grounding
            && let Some(model) = &config.grounded_model
        {
            match self
                .gemini
                .generate(std::slice::from_ref(model), request, true)
                .await
            {
                Ok(reply) => return Ok(reply),
                Err(error @ (AiError::Blocked(_) | AiError::Recitation)) => return Err(error),
                Err(error) => tracing::warn!(%error, "grounded model failed; falling back"),
            }
        }
        let models = match task {
            Task::Translate => &config.translate_models,
            Task::Chat => &config.chat_models,
        };
        let started = Instant::now();
        let result = self
            .gemini
            .generate(models, request, false)
            .await;
        metrics::histogram!("ai_latency_seconds", "task" => match task { Task::Translate => "translate", Task::Chat => "chat" })
            .record(started.elapsed().as_secs_f64());
        result
    }
}

pub fn start(app: &Arc<App>) {
    let app2 = Arc::clone(app);
    app.spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30 * 60));
        let cooldown_kept = app2.config.ai.cooldown.max(Duration::from_secs(60));
        loop {
            tokio::select! {
                () = app2.shutdown.cancelled() => break,
                _ = tick.tick() => {
                    let cutoff = db::now() - app2.config.ai.history_ttl.as_secs() as i64;
                    if let Err(error) = app2.db.call(move |c| db::ai::prune_turns(c, cutoff)).await {
                        tracing::warn!(%error, "failed to prune AI history");
                    }
                    let now = Instant::now();
                    app2.ai.shares.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                        .saved.retain(|_, s| now.duration_since(s.at) < SHARE_TTL);
                    app2.ai.cooldowns.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                        .retain(|_, at| now.duration_since(*at) < cooldown_kept);
                    app2.ai.cleared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                        .retain(|_, at| now.duration_since(*at) < CLEARED_KEPT);
                    let today = db::now().div_euclid(86_400);
                    app2.ai.daily.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                        .retain(|_, d| d.day == today);
                }
            }
        }
    });
}

/// Keeps line breaks and spacing, which matter for translating code.
pub fn strip_mention(raw: &str, bot: Id<UserMarker>) -> String {
    let plain = format!("<@{bot}>");
    let nick = format!("<@!{bot}>");
    raw.replace(&plain, "")
        .replace(&nick, "")
        .trim()
        .to_owned()
}

/// Hold the permit until the reply is done; the parts stay in memory until then.
async fn collect_parts<'a>(
    app: &'a App,
    requester: Id<UserMarker>,
    attachments: &[&Attachment],
) -> (
    Vec<Part>,
    usize,
    Option<tokio::sync::SemaphorePermit<'a>>,
) {
    let mime_of = |attachment: &Attachment| {
        attachment
            .content_type
            .as_deref()
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let wanted: Vec<(&Attachment, String)> = attachments
        .iter()
        .take(MAX_ATTACHMENTS)
        .map(|a| (*a, mime_of(a)))
        .filter(|(_, mime)| gemini::supported_mime(mime))
        .collect();
    let mut skipped = attachments.len().min(MAX_ATTACHMENTS) - wanted.len();
    // A video or zip must not hold one of the two media slots while its reply is generated.
    let permit = if wanted.is_empty() { None } else { app.ai.media.acquire().await.ok() };
    let mut parts = Vec::new();
    let mut budget = app.config.ai.max_inline_bytes;
    for (attachment, mime) in wanted {
        if attachment.size as usize > budget {
            skipped += 1;
            continue;
        }
        let bytes = match app
            .web
            .get(&attachment.url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(response) => match response.bytes().await {
                Ok(bytes) => Vec::from(bytes),
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            },
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        // Decoded in the image worker, so a crafted GIF can't take the bot down.
        let normalized = if let Some(real) = gemini::needs_conversion(&mime, &bytes) {
            match app
                .images
                .convert(real, &bytes, budget)
                .await
            {
                Ok(png) => Some(("image/png".to_owned(), png)),
                Err(error @ crate::image_worker::WorkerError::Crashed(_)) => {
                    tracing::warn!(user = requester.get(), %error, "attachment conversion crashed");
                    None
                }
                Err(error) => {
                    tracing::debug!(%error, "attachment not converted");
                    None
                }
            }
        } else {
            gemini::normalize_image(&mime, bytes)
        };
        match normalized {
            Some((mime, data)) if data.len() <= budget => {
                budget -= data.len();
                parts.push(Part::Inline { mime, data });
            }
            _ => skipped += 1,
        }
    }
    (parts, skipped, permit)
}

fn cleared_since(
    cleared: &Mutex<HashMap<Id<UserMarker>, Instant>>,
    user: Id<UserMarker>,
    asked: Instant,
) -> bool {
    cleared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&user)
        .is_some_and(|at| *at >= asked)
}

/// A `)` or space in a source URL would end the link early and let the rest render as markdown.
fn escape_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        match c {
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '[' => out.push_str("%5B"),
            ']' => out.push_str("%5D"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            ' ' => out.push_str("%20"),
            '\n' => out.push_str("%0A"),
            '\t' => out.push_str("%09"),
            '\r' => out.push_str("%0D"),
            '\\' => out.push_str("%5C"),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn escaped_len(url: &str) -> usize {
    url.len()
        + 2 * url
            .bytes()
            .filter(|b| b"()[]<> \n\t\r\\".contains(b))
            .count()
}

/// The model posts as the bot and prompts can steer it, so `[label](url)` shows as
/// `label (<url>)`.
fn unmask_links(text: &str) -> String {
    // Each pass is quadratic in a run of `[`, so long replies are only split.
    if text.len() > 16 * 1024 {
        return break_masks(text);
    }
    let mut text = unmask_once(text);
    // Nested links need a pass each; stop after three and let `break_masks` handle the rest.
    for _ in 0..3 {
        if !text.contains("](") {
            break;
        }
        let next = unmask_once(&text);
        if next == text {
            break;
        }
        text = next;
    }
    break_masks(&text)
}

/// Splits any leftover `](`, so whatever unmasking missed can't render as a link.
fn break_masks(text: &str) -> String {
    text.replace("](", "] (")
}

fn is_web(target: &str) -> bool {
    target
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
        || target
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
}

fn unmask_once(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            let label = &after[..close];
            let tail = &after[close + 2..];
            let end = tail.find(')')?;
            let target = tail[..end].trim();
            // `<url>` is how Discord writes a link without a preview.
            let url = target
                .strip_prefix('<')
                .and_then(|t| t.strip_suffix('>'))
                .unwrap_or(target);
            (!label.contains('[') && is_web(url)).then_some((
                label,
                url,
                open + 1 + close + 2 + end + 1,
            ))
        });
        match link {
            Some((label, url, consumed)) => {
                out.push_str(&rest[..open]);
                out.push_str(&format!("{label} (<{url}>)"));
                rest = &rest[consumed..];
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = &rest[open + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn source_host(uri: &str) -> String {
    reqwest::Url::parse(uri)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| ui::link_label(&ui::truncate(host, 80)))
        })
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "ลิงก์".to_owned())
}

fn reply_tail(reply: &Reply, footer: &str) -> (Option<String>, Option<String>, String) {
    // Long URLs could push past 4000 characters, and escaping can triple one.
    let lines: Vec<String> = reply
        .sources
        .iter()
        .filter_map(|s| {
            let url = escape_url(&s.uri);
            (url.len() <= 300).then_some((s, url))
        })
        .take(5)
        .enumerate()
        .map(|(i, (s, url))| {
            let label = ui::link_label(&ui::truncate(&s.title, 80));
            // `link_label` can leave nothing visible, and would alter a URL used as the title.
            let label = if s.title != s.uri && ui::shows_text(&label) {
                label
            } else {
                source_host(&s.uri)
            };
            format!("{}. [{label}]({url})", i + 1)
        })
        .collect();
    let sources = (!lines.is_empty()).then(|| format!("**แหล่งข้อมูล**\n{}", lines.join("\n")));
    // Thai text grows ninefold when URL-encoded, so long queries are dropped rather than linked.
    let queries: Vec<String> = reply
        .queries
        .iter()
        .map(|q| {
            (
                q,
                format!("https://www.google.com/search?q={}", urlencode(q)),
            )
        })
        .filter(|(_, url)| url.len() <= 300)
        .take(3)
        .map(|(q, url)| {
            format!(
                "[{}]({url})",
                ui::link_label(&ui::truncate(q, 60))
            )
        })
        .collect();
    let queries = (!queries.is_empty()).then(|| format!("ค้นหาด้วย Google: {}", queries.join(" · ")));
    (sources, queries, footer.to_owned())
}

fn group_chunks(chunks: Vec<String>, tail_len: usize) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut used = 0;
    for chunk in chunks {
        let len = chunk.chars().count();
        if !current.is_empty() && (current.len() == 2 || used + len > ui::MESSAGE_TEXT_BUDGET) {
            groups.push(std::mem::take(&mut current));
            used = 0;
        }
        used += len;
        current.push(chunk);
    }
    if !current.is_empty() {
        let total: usize = current
            .iter()
            .map(|c| c.chars().count())
            .sum();
        if current.len() > 1 && total + tail_len > ui::MESSAGE_TEXT_BUDGET {
            let last = current.pop().unwrap_or_default();
            groups.push(current);
            groups.push(vec![last]);
        } else {
            groups.push(current);
        }
    }
    // Sources fit the budget on their own, so they get their own message if needed.
    let last: usize = groups.last().map_or(0, |g| {
        g.iter()
            .map(|c| c.chars().count())
            .sum()
    });
    if last + tail_len > ui::MESSAGE_TEXT_BUDGET {
        groups.push(Vec::new());
    }
    groups
}

fn reply_cards(reply: &Reply, footer: &str, extra_buttons: Vec<Component>) -> Vec<Vec<Component>> {
    let mut chunks = ui::split_text(&unmask_links(&reply.text), ui::TEXT_BLOCK_MAX);
    if reply.truncated
        && let Some(last) = chunks.last_mut()
    {
        last.push_str(" …");
    }
    let (sources, queries, footer) = reply_tail(reply, footer);
    let tail_len = [sources.as_deref(), queries.as_deref(), Some(footer.as_str())]
        .into_iter()
        .flatten()
        .map(|t| t.chars().count() + 4)
        .sum();
    let groups = group_chunks(chunks, tail_len);
    let total = groups.len();
    let mut messages = Vec::with_capacity(total);
    for (index, group) in groups.into_iter().enumerate() {
        let mut card = Card::new(color::AI);
        let tail_only = group.is_empty();
        for chunk in group {
            card = card.text(chunk);
        }
        if index + 1 == total {
            if let Some(sources) = &sources {
                if !tail_only {
                    card = card.divider();
                }
                card = card.text(sources.clone());
            }
            if let Some(queries) = &queries {
                card = card.small(queries);
            }
            card = card.small(&footer);
            if !extra_buttons.is_empty() {
                card = card.buttons(extra_buttons.clone());
            }
        }
        messages.push(vec![card.build()]);
    }
    messages
}

fn urlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn error_text(error: &AiError) -> &'static str {
    match error {
        AiError::NotConfigured => "ยังไม่ได้ตั้งค่า AI",
        AiError::Blocked(_) => "AI ปฏิเสธคำขอนี้ (ระบบความปลอดภัย)",
        AiError::Recitation => "AI ปฏิเสธเพราะคล้ายเนื้อหาลิขสิทธิ์ ลองครอปภาพให้เล็กลง",
        AiError::Unavailable(_) => "AI ไม่ว่าง ลองใหม่ภายหลัง",
        AiError::Rejected(_) => "AI รับไฟล์หรือข้อความนี้ไม่ได้",
        AiError::Untranslated => "แปลไม่สำเร็จ ลองใหม่อีกครั้ง",
    }
}

pub fn spawn_on_message(app: &Arc<App>, message: &Message) {
    let app2 = Arc::clone(app);
    let message = message.clone();
    app.spawn(async move { on_message(&app2, &message).await });
}

pub async fn on_message(app: &Arc<App>, message: &Message) {
    if message.author.bot
        || !message
            .mentions
            .iter()
            .any(|m| m.id == app.bot_id)
        || !app.ai.configured()
    {
        return;
    }
    let busy = match app.ai.admit(
        message.author.id,
        app.config.ai.cooldown,
        app.config.ai.daily_limit,
    ) {
        Ok(busy) => busy,
        Err(refused @ Refused::DailyLimit { first: true }) => {
            let (app2, message) = (Arc::clone(app), message.clone());
            app.spawn_outbound("ai_limit", async move {
                send_reply(&app2, &message, &[refused.card()]).await;
            });
            return;
        }
        Err(_) => {
            let (app2, channel, id) = (Arc::clone(app), message.channel_id, message.id);
            app.spawn_outbound("ai_busy", async move {
                app2.react(app::Reaction::Busy, channel, id)
                    .await;
            });
            return;
        }
    };
    let _ = app
        .http
        .create_typing_trigger(message.channel_id)
        .await;

    let text = strip_mention(&message.content, app.bot_id);
    let referenced = message.referenced_message.as_deref();
    let has_context = referenced.is_some() || !message.attachments.is_empty();
    let translate = prompts::translate_request(&text, has_context)
        .filter(|r| has_context || r.source.is_some());

    let mut attachments: Vec<&Attachment> = message.attachments.iter().collect();
    if let Some(referenced) = referenced {
        attachments.extend(crate::discord::shown_attachments(referenced));
    }
    let (inline, skipped, _media) = collect_parts(app, message.author.id, &attachments).await;
    let author = app::display_name(&message.author).to_owned();
    // Without this the model is asked to translate nothing and makes something up.
    if let Some(request) = &translate
        && inline.is_empty()
        && request.source.is_none()
        && referenced.is_none_or(|r| message_text(r).is_empty())
    {
        send_reply(app, message, &[ui::error("ไม่มีข้อความหรือรูปให้แปล")]).await;
        return;
    }

    let (task, result, footer) =
        if let Some(prompts::TranslateRequest { source, instruction }) = translate {
            let mut parts = inline;
            if let Some(referenced) = referenced {
                let quoted = message_text(referenced);
                if !quoted.is_empty() {
                    parts.push(Part::Text(format!(
                        "Message to translate:\n{quoted}"
                    )));
                }
            }
            if let Some(source) = &source {
                parts.push(Part::Text(format!(
                    "Text to translate:\n{source}"
                )));
            }
            let mut ask = String::from("Translate the content above into Thai.");
            if !instruction.is_empty() {
                ask.push_str(&format!(
                    " Extra instruction from the user: {instruction}"
                ));
            }
            parts.push(Part::Text(ask));
            let request = Request {
                system: prompts::TRANSLATE,
                history: &[],
                parts,
                effort: Effort::Low,
                grounding: false,
                max_output_tokens: 8192,
                accept: instruction
                    .is_empty()
                    .then_some(prompts::looks_translated as fn(&str) -> bool),
            };
            let source_author = referenced.map(|r| app::display_name(&r.author).to_owned());
            let footer = match (source_author, source.is_some()) {
                (Some(name), _) => format!("แปลข้อความของ {}", ui::escape_markdown(&name)),
                (None, true) => "แปลข้อความ".to_owned(),
                (None, false) => "แปลภาพ".to_owned(),
            };
            (
                Task::Translate,
                app.ai
                    .run(app, Task::Translate, &request)
                    .await,
                footer,
            )
        } else {
            let user = message.author.id;
            let scope = Scope::Public(message.channel_id);
            let since = db::now() - app.config.ai.history_ttl.as_secs() as i64;
            let limit = app.config.ai.history_turns;
            let history: Vec<(bool, String)> = app
                .db
                .call(move |c| db::ai::load_turns(c, user, scope, since, limit))
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|t| (t.from_model, t.text))
                .collect();
            let persona = app
                .db
                .call(move |c| db::ai::get_prompt(c, user))
                .await
                .ok()
                .flatten();
            let bot_name = app
                .cache
                .current_user()
                .map(|u| u.name)
                .unwrap_or_else(|| "Uncle Bot".into());
            let system = prompts::chat(
                &bot_name,
                &author,
                &today(),
                &app.config.ai.extra_prompt,
                persona.as_deref(),
            );
            let mut parts = inline;
            if let Some(referenced) = referenced {
                let quoted = message_text(referenced);
                if !quoted.is_empty() {
                    parts.push(Part::Text(format!(
                        "(Replying to {}: {quoted})",
                        app::display_name(&referenced.author)
                    )));
                }
            }
            let web = with_search(app, &text, &mut parts).await;
            parts.push(Part::Text(if text.is_empty() {
                "(no text)".into()
            } else {
                text.clone()
            }));
            let request = Request {
                system: &system,
                history: &history,
                parts,
                effort: Effort::Low,
                grounding: web.is_empty() && app.config.ai.grounded_model.is_some(),
                max_output_tokens: 2048,
                accept: None,
            };
            let result = attach_sources(
                app.ai
                    .run(app, Task::Chat, &request)
                    .await,
                web,
            );
            if let Ok(reply) = &result {
                remember(
                    app,
                    user,
                    scope,
                    &text,
                    attachments.len(),
                    reply,
                    busy.admitted,
                )
                .await;
            }
            (Task::Chat, result, String::new())
        };

    let mut footer = footer;
    if result.is_ok() && skipped > 0 {
        if !footer.is_empty() {
            footer.push_str(" · ");
        }
        footer.push_str(&format!("ข้ามไฟล์ที่ไม่รองรับ {skipped} ไฟล์"));
    }
    let messages = match &result {
        Ok(reply) => reply_cards(reply, &footer, Vec::new()),
        Err(error) => {
            tracing::warn!(?task, %error, "AI request failed");
            vec![ui::error(error_text(error))]
        }
    };
    send_reply(app, message, &messages).await;
}

async fn send_reply(app: &App, message: &Message, messages: &[Vec<Component>]) {
    for (index, components) in messages.iter().enumerate() {
        let mut create = app
            .http
            .create_message(message.channel_id)
            .flags(MessageFlags::IS_COMPONENTS_V2);
        if index == 0 {
            // If moderation deleted the message, the answer must not post on its own and repeat it.
            create = create
                .reply(message.id)
                .fail_if_not_exists(true);
        }
        if let Err(error) = create.components(components).await {
            tracing::warn!(%error, "failed to send AI reply");
            break;
        }
    }
}

async fn with_search(app: &App, question: &str, parts: &mut Vec<Part>) -> Vec<gemini::Source> {
    let Some(query) = search::decide(app, question).await else {
        return Vec::new();
    };
    match search::search(app, &query).await {
        Ok(hits) if !hits.is_empty() => {
            metrics::counter!("ai_searches_total").increment(1);
            parts.push(search::context_part(&query, &hits));
            search::sources(&hits)
        }
        Ok(_) => Vec::new(),
        Err(error) => {
            tracing::warn!(%error, "web search failed");
            Vec::new()
        }
    }
}

fn attach_sources(
    result: Result<Reply, AiError>,
    sources: Vec<gemini::Source>,
) -> Result<Reply, AiError> {
    result.map(|mut reply| {
        if reply.sources.is_empty() {
            reply.sources = sources;
        }
        reply
    })
}

async fn remember(
    app: &App,
    user: Id<UserMarker>,
    scope: Scope,
    text: &str,
    attachments: usize,
    reply: &Reply,
    asked: Instant,
) {
    // An empty turn replayed as history may be rejected by the API, breaking the channel's chat.
    let mut question =
        if text.trim().is_empty() { "(no text)".to_owned() } else { text.to_owned() };
    if attachments > 0 {
        question.push_str(&format!(" [แนบไฟล์ {attachments} ไฟล์]"));
    }
    let answer = if reply.queries.is_empty() {
        reply.text.clone()
    } else {
        "(คำตอบจากการค้นเว็บ ไม่ได้บันทึกเนื้อหา)".to_owned()
    };
    let keep = app.config.ai.history_turns;
    let now = db::now();
    let cleared = Arc::clone(&app.ai.cleared);
    if let Err(error) = app
        .db
        .call(move |c| {
            // On the database thread: a clear marks before queueing its delete, so either this
            // sees the mark or the delete runs after.
            if cleared_since(&cleared, user, asked) {
                return Ok(());
            }
            db::ai::append_exchange(c, user, scope, &question, &answer, now, keep)
        })
        .await
    {
        tracing::warn!(%error, "failed to store AI history");
    }
}

fn message_text(message: &Message) -> String {
    let mut text = crate::discord::shown_content(message)
        .trim()
        .to_owned();
    for embed in &message.embeds {
        for part in [embed.title.as_deref(), embed.description.as_deref()]
            .into_iter()
            .flatten()
        {
            push_line(&mut text, part);
        }
    }
    // Relay copies and the bot's own cards keep their text in Components V2.
    component_text(&message.components, &mut text);
    ui::truncate(&text, 6000)
}

fn component_text(components: &[Component], text: &mut String) {
    for component in components {
        match component {
            Component::TextDisplay(display) => push_line(text, &display.content),
            Component::Container(container) => component_text(&container.components, text),
            Component::Section(section) => component_text(&section.components, text),
            _ => {}
        }
    }
}

fn push_line(text: &mut String, part: &str) {
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(part);
}

fn today() -> String {
    jiff::Zoned::now().date().to_string()
}

pub async fn talk(
    app: &Arc<App>,
    interaction: &Interaction,
    text: String,
    attachment: Option<Attachment>,
) -> anyhow::Result<()> {
    let Some(user) = interaction.author() else { return Ok(()) };
    if !app.ai.configured() {
        return reply::respond(app, interaction, ui::error("ยังไม่ได้ตั้งค่า AI"), true).await;
    }
    let busy = match app.ai.admit(
        user.id,
        app.config.ai.cooldown,
        app.config.ai.daily_limit,
    ) {
        Ok(busy) => busy,
        Err(refused) => return reply::respond(app, interaction, refused.card(), true).await,
    };
    reply::defer(app, interaction, true).await?;
    let user_id = user.id;
    let since = db::now() - app.config.ai.history_ttl.as_secs() as i64;
    let limit = app.config.ai.history_turns;
    let history: Vec<(bool, String)> = app
        .db
        .call(move |c| db::ai::load_turns(c, user_id, Scope::Private, since, limit))
        .await?
        .into_iter()
        .map(|t| (t.from_model, t.text))
        .collect();
    let persona = app
        .db
        .call(move |c| db::ai::get_prompt(c, user_id))
        .await?;
    let bot_name = app
        .cache
        .current_user()
        .map(|u| u.name)
        .unwrap_or_else(|| "Uncle Bot".into());
    let system = prompts::chat(
        &bot_name,
        app::display_name(user),
        &today(),
        &app.config.ai.extra_prompt,
        persona.as_deref(),
    );
    let attachments: Vec<&Attachment> = attachment.iter().collect();
    let (mut parts, skipped, _media) = collect_parts(app, user.id, &attachments).await;
    let web = with_search(app, &text, &mut parts).await;
    parts.push(Part::Text(text.clone()));
    let request = Request {
        system: &system,
        history: &history,
        parts,
        effort: Effort::Low,
        grounding: web.is_empty() && app.config.ai.grounded_model.is_some(),
        max_output_tokens: 2048,
        accept: None,
    };
    let messages = match attach_sources(
        app.ai
            .run(app, Task::Chat, &request)
            .await,
        web,
    ) {
        Ok(reply) => {
            remember(
                app,
                user_id,
                Scope::Private,
                &text,
                attachments.len(),
                &reply,
                busy.admitted,
            )
            .await;
            let mut footer = String::from("ใช้ /ai clear เพื่อเริ่มหัวข้อใหม่");
            if skipped > 0 {
                footer.push_str(" · ไฟล์แนบไม่รองรับ");
            }
            reply_cards(&reply, &footer, Vec::new())
        }
        Err(error) => vec![ui::error(error_text(&error))],
    };
    post_pages(app, interaction, &messages)
        .await
        .map(drop)
}

/// After the first page, failures are only logged so the error card doesn't replace it.
async fn post_pages(
    app: &Arc<App>,
    interaction: &Interaction,
    messages: &[Vec<Component>],
) -> anyhow::Result<bool> {
    let Some((first, rest)) = messages.split_first() else {
        return Ok(true);
    };
    reply::edit(app, interaction, first).await?;
    for components in rest {
        if let Err(error) = reply::followup(app, interaction, components, true).await {
            tracing::warn!(%error, "AI reply page failed");
            return Ok(false);
        }
    }
    Ok(true)
}

pub async fn translate_message(
    app: &Arc<App>,
    interaction: &Interaction,
    target: &Message,
) -> anyhow::Result<()> {
    let (Some(user), Some(channel)) = (
        interaction.author(),
        interaction
            .channel
            .as_ref()
            .map(|c| c.id),
    ) else {
        return Ok(());
    };
    if !app.ai.configured() {
        return reply::respond(app, interaction, ui::error("ยังไม่ได้ตั้งค่า AI"), true).await;
    }
    let _busy = match app.ai.admit(
        user.id,
        app.config.ai.cooldown,
        app.config.ai.daily_limit,
    ) {
        Ok(busy) => busy,
        Err(refused) => return reply::respond(app, interaction, refused.card(), true).await,
    };
    reply::defer(app, interaction, true).await?;
    let attachments: Vec<&Attachment> = crate::discord::shown_attachments(target)
        .iter()
        .collect();
    let (mut parts, skipped, _media) = collect_parts(app, user.id, &attachments).await;
    let source = message_text(target);
    if source.is_empty() && parts.is_empty() {
        let text = if skipped > 0 {
            "ไฟล์แนบไม่รองรับ"
        } else {
            "ไม่มีข้อความหรือรูปให้แปล"
        };
        reply::edit(app, interaction, &ui::error(text)).await?;
        return Ok(());
    }
    if !source.is_empty() {
        parts.push(Part::Text(format!(
            "Message to translate:\n{source}"
        )));
    }
    parts.push(Part::Text(
        "Translate the content above into Thai.".into(),
    ));
    let request = Request {
        system: prompts::TRANSLATE,
        history: &[],
        parts,
        effort: Effort::Low,
        grounding: false,
        max_output_tokens: 8192,
        accept: Some(prompts::looks_translated),
    };
    let reply = match app
        .ai
        .run(app, Task::Translate, &request)
        .await
    {
        Ok(reply) => reply,
        Err(error) => {
            reply::edit(app, interaction, &ui::error(error_text(&error))).await?;
            return Ok(());
        }
    };
    let jump = interaction.guild_id.map(|g| {
        format!(
            "https://discord.com/channels/{g}/{}/{}",
            target.channel_id, target.id
        )
    });
    let mut footer = format!(
        "แปลข้อความของ {}",
        ui::escape_markdown(app::display_name(&target.author))
    );
    if skipped > 0 {
        footer.push_str(&format!(" · ข้ามไฟล์ที่ไม่รองรับ {skipped} ไฟล์"));
    }
    let mut buttons = Vec::new();
    let key = interaction.id.get();
    if can_post(interaction)
        && !quarantined(app, interaction, user.id).await
        && keep_share(
            &mut app
                .ai
                .shares
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            key,
            Share {
                at: Instant::now(),
                owner: user.id,
                channel,
                text: reply.text.clone(),
                source: jump.clone(),
            },
        )
    {
        buttons.push(ui::button_with(
            ButtonStyle::Primary,
            format!("{SHARE_PREFIX}{key}"),
            "แชร์ในห้อง",
            false,
        ));
    }
    let shared = !buttons.is_empty();
    let messages = reply_cards(&reply, &footer, buttons);
    let posted = post_pages(app, interaction, &messages).await;
    // The button is on the last page; a share nobody can click would only take room.
    if shared && !matches!(posted, Ok(true)) {
        app.ai
            .shares
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .saved
            .remove(&key);
    }
    posted.map(drop)
}

/// One per member, `SHARE_BYTES` in all, oldest dropped first. False if it can't fit beside
/// posts in flight.
fn keep_share(shares: &mut Shares, key: u64, share: Share) -> bool {
    // Checked first, so a refused share leaves the member's previous one usable.
    if shares.posting + share.bytes() > SHARE_BYTES {
        return false;
    }
    shares
        .saved
        .retain(|_, s| s.owner != share.owner && s.at.elapsed() < SHARE_TTL);
    let mut total = shares.posting
        + share.bytes()
        + shares
            .saved
            .values()
            .map(Share::bytes)
            .sum::<usize>();
    while total > SHARE_BYTES {
        let Some((&oldest, _)) = shares
            .saved
            .iter()
            .min_by_key(|(_, s)| s.at)
        else {
            break;
        };
        if let Some(dropped) = shares.saved.remove(&oldest) {
            total -= dropped.bytes();
        }
    }
    shares.saved.insert(key, share);
    true
}

/// Moves a share to `posting` under the same lock, so a new share never misses its bytes.
fn take_share(shares: &mut Shares, key: u64, valid: impl Fn(&Share) -> bool) -> Option<Share> {
    let share = shares
        .saved
        .remove(&key)
        .filter(valid)?;
    shares.posting += share.bytes();
    Some(share)
}

fn share_post(share: Share, user: Id<UserMarker>) -> Vec<Vec<Component>> {
    let reply = Reply { text: share.text, ..Reply::default() };
    let footer = format!("แปลตามคำขอของ <@{user}>");
    let buttons = share
        .source
        .map(|url| vec![ui::link(url, "ข้อความต้นทาง")])
        .unwrap_or_default();
    reply_cards(&reply, &footer, buttons)
}

/// Quarantined members keep `/ai` but can't post as the bot. A failed lookup counts as quarantined.
async fn quarantined(app: &App, interaction: &Interaction, user: Id<UserMarker>) -> bool {
    let Some(guild) = interaction.guild_id else { return true };
    match app
        .db
        .call(move |c| db::quarantine::get(c, guild, user))
        .await
    {
        Ok(record) => record.is_some(),
        Err(error) => {
            tracing::warn!(%error, "could not check quarantine before a share");
            true
        }
    }
}

/// Shares post as the bot, so only where both the member and the bot can post.
fn can_post(interaction: &Interaction) -> bool {
    let needed = if interaction
        .channel
        .as_ref()
        .is_some_and(|c| c.kind.is_thread())
    {
        Permissions::SEND_MESSAGES_IN_THREADS
    } else {
        Permissions::SEND_MESSAGES
    };
    let allowed = |p: Permissions| {
        p.contains(Permissions::ADMINISTRATOR) || p.contains(needed | Permissions::VIEW_CHANNEL)
    };
    interaction
        .member
        .as_ref()
        .and_then(|m| m.permissions)
        .is_some_and(allowed)
        && interaction
            .app_permissions
            .is_none_or(allowed)
}

pub async fn on_share(
    app: &Arc<App>,
    interaction: &Interaction,
    custom_id: &str,
) -> anyhow::Result<()> {
    let Some(user) = interaction.author_id() else { return Ok(()) };
    let key = custom_id
        .strip_prefix(SHARE_PREFIX)
        .and_then(|k| k.parse::<u64>().ok())
        .unwrap_or_default();
    let channel = interaction
        .channel
        .as_ref()
        .map(|c| c.id);
    let valid =
        |s: &Share| s.owner == user && Some(s.channel) == channel && s.at.elapsed() < SHARE_TTL;
    let found = app
        .ai
        .shares
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .saved
        .get(&key)
        .is_some_and(valid);
    if !found {
        return reply::respond(
            app,
            interaction,
            ui::error("หมดเวลาแชร์ แปลใหม่อีกครั้ง"),
            true,
        )
        .await;
    }
    if !can_post(interaction) {
        return reply::respond(
            app,
            interaction,
            ui::error("คุณส่งข้อความในห้องนี้ไม่ได้"),
            true,
        )
        .await;
    }
    // The error path would replace the translation, and the ack may have landed; keep the share.
    if let Err(error) = reply::defer_update(app, interaction).await {
        tracing::warn!(%error, "share click not acknowledged");
        return Ok(());
    }
    if quarantined(app, interaction, user).await {
        if let Err(error) = reply::followup(
            app,
            interaction,
            &ui::error("แชร์ไม่ได้ระหว่างถูกกักกัน"),
            true,
        )
        .await
        {
            tracing::warn!(%error, "share refusal not sent");
        }
        return Ok(());
    }
    // Taken only once the click is acknowledged, so a failed acknowledgement keeps the share.
    let share = take_share(
        &mut app
            .ai
            .shares
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        key,
        valid,
    );
    // Gone means another click on the same card took it and edits the card itself.
    let Some(share) = share else { return Ok(()) };
    let guard = Posting { app: Arc::clone(app), bytes: share.bytes() };
    let channel = share.channel;
    let cards = share_post(share, user);
    let poster = Arc::clone(app);
    // Post outside the handler to free the interaction place.
    let posting = app.spawn_outbound("share", async move {
        let _guard = guard;
        for components in cards {
            if let Err(error) = poster
                .http
                .create_message(channel)
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .components(&components)
                .await
            {
                tracing::warn!(%error, "share post failed");
                return;
            }
        }
    });
    let card = match posting {
        true => ui::success("แชร์แล้ว", "กำลังโพสต์คำแปลในห้อง"),
        false => ui::error("บอทยุ่ง แปลใหม่อีกครั้ง"),
    };
    if let Err(error) = reply::edit(app, interaction, &card).await {
        tracing::warn!(%error, "share result not shown");
    }
    Ok(())
}

pub async fn set_prompt(app: &App, user: Id<UserMarker>, prompt: String) -> anyhow::Result<()> {
    app.ai.history_cleared(user);
    let now = db::now();
    app.db
        .call(move |c| {
            db::ai::set_prompt(c, user, &prompt, now)?;
            db::ai::clear_turns(c, user)
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_labels_that_show_nothing_use_the_host() {
        let source =
            |title: &str, uri: &str| gemini::Source { title: title.into(), uri: uri.into() };
        let fandom = "https://warframe.fandom.com/wiki/Hate_(Incarnon)";
        let reply = Reply {
            sources: vec![
                source("*****", "https://a.example/x"),
                source(
                    &format!("{}Real title", "\u{3164}".repeat(80)),
                    "https://b.example/",
                ),
                source(fandom, fandom),
                source(
                    "",
                    &format!("https://{}.example/", "c".repeat(200)),
                ),
                source("หน้าไทย", "https://d.example/"),
            ],
            ..Reply::default()
        };
        let sources = reply_tail(&reply, "").0.unwrap();
        let labels: Vec<&str> = sources
            .lines()
            .skip(1)
            .map(|line| &line[line.find('[').unwrap() + 1..line.find("](").unwrap()])
            .collect();
        assert_eq!(
            [labels[0], labels[1], labels[2], labels[4]],
            ["a.example", "b.example", "warframe.fandom.com", "หน้าไทย"]
        );
        assert_eq!(labels[3].chars().count(), 80);
        assert!(labels[3].starts_with("ccc") && labels[3].ends_with('…'));
    }

    #[test]
    fn shares_are_one_per_member_and_bounded() {
        let share = |owner: u64, at: Instant, bytes: usize| Share {
            at,
            owner: Id::new(owner),
            channel: Id::new(1),
            text: "x".repeat(bytes),
            source: None,
        };
        let now = Instant::now();
        let mut shares = Shares::default();
        keep_share(&mut shares, 1, share(10, now, 100));
        keep_share(&mut shares, 2, share(10, now, 100));
        assert_eq!(
            shares
                .saved
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [2]
        );

        if let Some(stale) = now.checked_sub(SHARE_TTL + Duration::from_secs(1)) {
            shares
                .saved
                .insert(3, share(11, stale, 100));
            keep_share(&mut shares, 4, share(12, now, 100));
            assert!(!shares.saved.contains_key(&3));
        }

        // Text whose share is charged a third of the room.
        let third = (SHARE_BYTES / 3 - PAGE_BODY_BYTES - 2048) * 2 / 3;
        assert!(share(1, now, third).bytes() <= SHARE_BYTES / 3);
        shares.saved.clear();
        for owner in 0..6 {
            let at = now + Duration::from_millis(owner);
            assert!(keep_share(
                &mut shares,
                owner,
                share(owner + 20, at, third)
            ));
        }
        let mut kept: Vec<_> = shares.saved.keys().copied().collect();
        kept.sort_unstable();
        assert_eq!(kept, [3, 4, 5]);
        assert!(
            shares
                .saved
                .values()
                .map(Share::bytes)
                .sum::<usize>()
                <= SHARE_BYTES
        );

        // Posts in flight count too.
        shares.posting = SHARE_BYTES - share(1, now, third).bytes();
        assert!(keep_share(&mut shares, 6, share(26, now, third)));
        assert_eq!(shares.saved.len(), 1);
        assert!(!keep_share(
            &mut shares,
            7,
            share(27, now, third + 1)
        ));
        assert!(shares.saved.contains_key(&6) && !shares.saved.contains_key(&7));
        // A refused share keeps the member's previous one.
        assert!(!keep_share(
            &mut shares,
            8,
            share(26, now, third + 1)
        ));
        assert!(shares.saved.contains_key(&6));
    }

    #[test]
    fn masked_links_show_their_address() {
        assert_eq!(
            unmask_links(
                "ดู [Wiki](https://wiki.warframe.com/w/Baro) และ [x](https://phish.example/a)"
            ),
            "ดู Wiki (<https://wiki.warframe.com/w/Baro>) และ x (<https://phish.example/a>)"
        );
        assert_eq!(
            unmask_links("array[0](1) [not a link]"),
            "array[0] (1) [not a link]"
        );
        assert_eq!(
            unmask_links("[a](<https://x.example>)"),
            "a (<https://x.example>)"
        );
        assert_eq!(
            unmask_links("[a](HTTPS://x.example)"),
            "a (<HTTPS://x.example>)"
        );
        assert_eq!(
            unmask_links("[[a](https://x.example)](https://evil.example)"),
            "a (<https://x.example>) (<https://evil.example>)"
        );
        for masked in [
            "[Claim [here](now)](https://phish.example)",
            "[[y](z)](https://evil.example)",
            "[Wiki](<https://evil.example> \"x\")",
            "[a](b](https://evil.example)",
            "[a]( <HTTP://evil.example>)",
            "[a](\u{FEFF}https://evil.example)",
            "[a](<\u{FEFF}https://evil.example>)",
            "[a](https\\://evil.example)",
            "[a](https:\\/\\/evil.example)",
            "[Claim [here](now)](https\\://phish.example)",
        ] {
            let shown = unmask_links(masked).to_ascii_lowercase();
            assert!(
                !shown.contains("](h") && !shown.contains("](<") && !shown.contains("]( "),
                "{masked} -> {shown}"
            );
        }
        let depth = 1500;
        let nested = format!(
            "{}a{}",
            "[".repeat(depth),
            "](https://evil.example)".repeat(depth)
        );
        assert!(!unmask_links(&nested).contains("]("));
        assert_eq!(
            escape_url("https://a.b/x_(y) z"),
            "https://a.b/x_%28y%29%20z"
        );
        let url = "https://a.b/[x]<y>\n\t\r\\ ก";
        assert_eq!(
            escape_url(url),
            "https://a.b/%5Bx%5D%3Cy%3E%0A%09%0D%5C%20ก"
        );
        assert_eq!(escaped_len(url), escape_url(url).len());
    }

    #[test]
    fn card_text_is_read_for_translation() {
        let card = Card::new(color::AI)
            .title("ประกาศ")
            .text("メンテナンス中")
            .section_button(
                "セクション".to_owned(),
                ui::button_with(ButtonStyle::Secondary, "x".to_owned(), "b", false),
            )
            .build();
        let mut text = String::from("content");
        component_text(&[card], &mut text);
        for part in ["content", "ประกาศ", "メンテナンス中", "セクション"] {
            assert!(
                text.contains(part),
                "{part} missing from {text:?}"
            );
        }
    }

    #[test]
    fn strips_both_mention_forms() {
        let bot = Id::new(42);
        assert_eq!(strip_mention("<@42> แปล", bot), "แปล");
        assert_eq!(
            strip_mention("hi <@!42>there <@7>", bot),
            "hi there <@7>"
        );
        assert_eq!(
            strip_mention("<@42> แปล\n一行目\n二行目", bot),
            "แปล\n一行目\n二行目"
        );
    }

    #[test]
    fn replies_with_sources_stay_within_message_limits() {
        // Redirect URLs near the cap triple when escaped, so they are dropped.
        let sources: Vec<gemini::Source> = (0..5)
            .map(|i| gemini::Source {
                title: "t".repeat(200),
                uri: format!("https://example.com/{}", "(".repeat(270 + i)),
            })
            .chain((0..8).map(|i| gemini::Source {
                title: "t".repeat(200),
                uri: format!("https://example.com/{}", "p".repeat(270 + i)),
            }))
            .collect();
        let queries: Vec<String> = (0..5)
            .map(|i| {
                format!("warframe baro ki'teer inventory and schedule this week, prime vault {i}")
            })
            .chain(["ก".repeat(400)])
            .collect();
        for len in [10, 1_800, 1_900, 3_700, 3_900, 7_000, 12_000] {
            let reply = Reply {
                text: "ก ".repeat(len / 2),
                model: "m".into(),
                sources: sources.clone(),
                queries: queries.clone(),
                ..Reply::default()
            };
            for message in reply_cards(
                &reply,
                "🤖 footer",
                vec![ui::link("https://a.b", "x")],
            ) {
                ui::validate_message(&message).unwrap();
            }
            let reply = Reply { sources: Vec::new(), ..reply };
            for message in reply_cards(&reply, "f", Vec::new()) {
                ui::validate_message(&message).unwrap();
            }
        }
    }

    #[test]
    fn long_replies_split_into_messages_of_two_blocks() {
        let reply = Reply { text: "ก ".repeat(3000), model: "m".into(), ..Reply::default() };
        let messages = reply_cards(&reply, "f", Vec::new());
        assert_eq!(
            messages.len(),
            1 + (ui::split_text(&reply.text, ui::TEXT_BLOCK_MAX).len() - 1) / 2
        );
        assert!(
            messages
                .iter()
                .all(|m| ui::count_components(m) <= 40)
        );
    }

    #[test]
    fn urlencode_handles_thai() {
        assert_eq!(urlencode("a b"), "a+b");
        assert_eq!(urlencode("ก"), "%E0%B8%81");
    }

    #[tokio::test]
    async fn overlapping_request_is_refused_without_deadlock() {
        let app = crate::app::test_app().await;
        let user = Id::new(42);
        let first = app.ai.admit(user, Duration::ZERO, 0);
        assert!(first.is_ok());
        let (tx, rx) = std::sync::mpsc::channel();
        let shared = Arc::clone(&app);
        std::thread::spawn(move || {
            let _ = tx.send(
                shared
                    .ai
                    .admit(user, Duration::ZERO, 0)
                    .is_err(),
            );
        });
        let Ok(refused) = rx.recv_timeout(Duration::from_secs(5)) else {
            // The stuck thread holds `busy`; dropping `first` would block on it too.
            std::mem::forget(first);
            panic!("admit deadlocked");
        };
        assert!(refused);
        drop(first);
        assert!(
            app.ai
                .admit(user, Duration::ZERO, 0)
                .is_ok()
        );
    }

    #[tokio::test]
    async fn clearing_history_drops_replies_still_in_flight() {
        let app = crate::app::test_app().await;
        let user = Id::new(42);
        let stored = || {
            let app = Arc::clone(&app);
            async move {
                app.db
                    .call(move |c| db::ai::load_turns(c, user, Scope::Private, 0, 50))
                    .await
                    .unwrap()
                    .len()
            }
        };
        let reply = Reply { text: "answer".into(), ..Reply::default() };
        let asked = app
            .ai
            .admit(user, Duration::ZERO, 0)
            .unwrap()
            .admitted;
        app.ai.history_cleared(user);
        remember(
            &app,
            user,
            Scope::Private,
            "old question",
            0,
            &reply,
            asked,
        )
        .await;
        assert_eq!(stored().await, 0);
        std::thread::sleep(Duration::from_millis(2));
        let later = app
            .ai
            .admit(user, Duration::ZERO, 0)
            .unwrap()
            .admitted;
        remember(
            &app,
            user,
            Scope::Private,
            "new question",
            0,
            &reply,
            later,
        )
        .await;
        assert_eq!(stored().await, 2);
    }

    #[tokio::test]
    async fn a_member_is_stopped_at_the_daily_limit_and_told_once() {
        let app = crate::app::test_app().await;
        let user = Id::new(5);
        for _ in 0..2 {
            drop(
                app.ai
                    .admit(user, Duration::ZERO, 2)
                    .unwrap(),
            );
        }
        assert!(matches!(
            app.ai.admit(user, Duration::ZERO, 2),
            Err(Refused::DailyLimit { first: true })
        ));
        assert!(matches!(
            app.ai.admit(user, Duration::ZERO, 2),
            Err(Refused::DailyLimit { first: false })
        ));
        assert!(
            app.ai
                .admit(Id::new(6), Duration::ZERO, 2)
                .is_ok()
        );
    }

    #[tokio::test]
    async fn requests_across_users_are_capped() {
        let app = crate::app::test_app().await;
        let mut running: Vec<_> = (1..=MAX_RUNNING as u64)
            .map(|id| {
                app.ai
                    .admit(Id::new(id), Duration::ZERO, 0)
                    .expect("slot")
            })
            .collect();
        let next = Id::new(100);
        assert!(matches!(
            app.ai.admit(next, Duration::ZERO, 0),
            Err(Refused::Busy)
        ));
        running.pop();
        assert!(
            app.ai
                .admit(next, Duration::ZERO, 0)
                .is_ok()
        );
    }
}
