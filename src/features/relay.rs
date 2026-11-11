use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use twilight_model::{
    channel::{
        Message,
        message::{Component, MessageFlags, MessageType},
    },
    id::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker},
    },
};

use crate::{
    app::{self, App},
    discord::{
        errors,
        ui::{self, Card, color},
    },
    guild::ChannelKind,
};

const REPLICA_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_REPLICAS: usize = 4096;
/// Deleted originals whose relay hadn't started. Counted, not timed: a scan can hold a message
/// for any length of time.
const MAX_GONE: usize = 4096;

/// A few workers drain deletes, so a flood costs 16 bytes per copy, not a task.
const MAX_PENDING_DELETES: usize = 16384;
const DELETE_WORKERS: usize = 4;

type Copy = (Id<ChannelMarker>, Id<MessageMarker>);
type Replicas = Vec<Copy>;

#[derive(Default)]
struct Copies {
    posted: HashMap<Id<MessageMarker>, (Instant, Replicas)>,
    /// `Some(moderation)` once the original is deleted, so copies still posting get deleted.
    posting: HashMap<Id<MessageMarker>, Option<bool>>,
    gone: HashSet<Id<MessageMarker>>,
    gone_order: VecDeque<Id<MessageMarker>>,
}

impl Copies {
    fn forget_gone(&mut self) {
        while self.gone_order.len() > MAX_GONE
            && let Some(id) = self.gone_order.pop_front()
        {
            self.gone.remove(&id);
        }
    }
}

pub struct Relay {
    copies: Mutex<Copies>,
    deletes: app::Pending<Copy>,
}

impl Default for Relay {
    fn default() -> Self {
        Self {
            copies: Mutex::default(),
            deletes: app::Pending::new(DELETE_WORKERS, MAX_PENDING_DELETES),
        }
    }
}

impl Relay {
    fn copies(&self) -> std::sync::MutexGuard<'_, Copies> {
        self.copies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Records a copy at once, so a delete that arrives mid-relay reaches it. Returns how to
    /// delete the copy if the original is already gone.
    fn remember(&self, original: Id<MessageMarker>, copy: Copy) -> Option<bool> {
        let mut copies = self.copies();
        if let Some(&Some(moderation)) = copies.posting.get(&original) {
            return Some(moderation);
        }
        let map = &mut copies.posted;
        if let Some((_, copies)) = map.get_mut(&original) {
            copies.push(copy);
            return None;
        }
        let now = Instant::now();
        map.retain(|_, (at, _)| now.duration_since(*at) < REPLICA_TTL);
        if map.len() >= MAX_REPLICAS
            && let Some(oldest) = map
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(id, _)| *id)
        {
            map.remove(&oldest);
        }
        map.insert(original, (now, vec![copy]));
        None
    }

    fn take(&self, original: Id<MessageMarker>, moderation: bool) -> Option<Replicas> {
        let mut copies = self.copies();
        if let Some(gone) = copies.posting.get_mut(&original) {
            *gone = Some(moderation || gone.unwrap_or(false));
            return copies
                .posted
                .remove(&original)
                .map(|(_, r)| r);
        }
        let posted = copies
            .posted
            .remove(&original)
            .map(|(_, r)| r);
        if posted.is_none() && copies.gone.insert(original) {
            copies.gone_order.push_back(original);
            copies.forget_gone();
        }
        posted
    }

    fn deleted(&self, original: Id<MessageMarker>) -> bool {
        matches!(
            self.copies().posting.get(&original),
            Some(Some(_))
        )
    }
}

/// A guard, so a relay task cut off by its timeout still clears its mark.
struct Posting<'a> {
    relay: &'a Relay,
    original: Id<MessageMarker>,
}

impl<'a> Posting<'a> {
    fn start(relay: &'a Relay, original: Id<MessageMarker>) -> Option<Self> {
        let mut copies = relay.copies();
        if copies.gone.contains(&original) {
            return None;
        }
        copies.posting.insert(original, None);
        Some(Self { relay, original })
    }
}

impl Drop for Posting<'_> {
    fn drop(&mut self) {
        self.relay
            .copies()
            .posting
            .remove(&self.original);
    }
}

pub fn relay_kinds(
    app: &App,
    guild: Id<GuildMarker>,
    channel: Id<ChannelMarker>,
) -> Vec<ChannelKind> {
    app.guild(guild)
        .kinds_of(channel)
        .filter(|k| k.is_relay())
        .collect()
}

/// Takes the message so it's freed once the card is rendered.
pub async fn on_message(app: &Arc<App>, message: Message) {
    let Some(guild) = message.guild_id else {
        return;
    };
    // System messages, stickers and polls have nothing the card can show.
    if message.author.bot
        || message
            .author
            .system
            .unwrap_or_default()
        || !matches!(
            message.kind,
            MessageType::Regular | MessageType::Reply
        )
        || (crate::discord::shown_content(&message)
            .trim()
            .is_empty()
            && crate::discord::shown_attachments(&message).is_empty())
    {
        return;
    }
    let kinds = relay_kinds(app, guild, message.channel_id);
    if kinds.is_empty() {
        return;
    }
    let components = render(app, guild, &message);
    let (original, source) = (message.id, message.channel_id);
    drop(message);
    let Some(_posting) = Posting::start(&app.relay, original) else {
        return;
    };

    let mut targets: Vec<Id<ChannelMarker>> = Vec::new();
    for kind in kinds {
        for (target_guild, channel) in app.guilds.with_channel(kind) {
            if target_guild != guild && !targets.contains(&channel) {
                targets.push(channel);
            }
        }
    }

    let mut sent = 0;
    for target in targets {
        if app.relay.deleted(original) {
            break;
        }
        match app
            .http
            .create_message(target)
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(&components)
            .await
        {
            Ok(response) => {
                if let Ok(copy) = crate::discord::read(response).await {
                    match app
                        .relay
                        .remember(original, (target, copy.id))
                    {
                        None => sent += 1,
                        Some(moderation) => {
                            queue_deletes(app, vec![(target, copy.id)], moderation);
                        }
                    }
                }
            }
            Err(error) => {
                tracing::warn!(channel = target.get(), %error, "relay send failed");
            }
        }
    }
    drop(components);

    metrics::counter!("relay_messages_total").increment(sent);
    if sent > 0 && !app.relay.deleted(original) {
        app.react(crate::app::Reaction::Relayed, source, original)
            .await;
    }
}

/// Runs outside the event slots, since deletes wait on per-channel rate limits.
pub fn delete_copies(app: &Arc<App>, originals: &[Id<MessageMarker>], moderation: bool) {
    let copies: Replicas = originals
        .iter()
        .filter_map(|&original| app.relay.take(original, moderation))
        .flatten()
        .collect();
    if !copies.is_empty() {
        queue_deletes(app, copies, moderation);
    }
}

/// Moderation deletes go first and are never dropped.
fn queue_deletes(app: &Arc<App>, copies: Replicas, moderation: bool) {
    let (start, dropped) = app
        .relay
        .deletes
        .push(copies, moderation);
    if dropped > 0 {
        metrics::counter!("outbound_dropped_total", "kind" => "relay_delete")
            .increment(dropped as u64);
    }
    for _ in 0..start {
        let app2 = Arc::clone(app);
        app.spawn(async move {
            let mut worker = app2.relay.deletes.worker();
            while let Some((channel, message)) = worker.take() {
                if app2.shutdown.is_cancelled() {
                    continue;
                }
                if let Err(error) = app2
                    .http
                    .delete_message(channel, message)
                    .await
                    && !errors::is(&error, errors::UNKNOWN_MESSAGE)
                {
                    tracing::warn!(channel = channel.get(), %error, "failed to delete relay replica");
                }
            }
        });
    }
}

/// Discord caps a message at 4000 characters, and attachment URLs are long.
const RELAY_TEXT_BUDGET: usize = 3600;
const RELAY_MAX_FILES: usize = 5;

fn relay_text(author: &str, content: &str, files: &[(&str, &str)]) -> (String, String) {
    let mut lines: Vec<String> = files
        .iter()
        .take(RELAY_MAX_FILES)
        .map(|(name, url)| {
            let name = ui::link_label(&ui::truncate(name, 60));
            if url.len() <= 400 {
                format!("ไฟล์แนบ: [{name}]({url})")
            } else {
                format!("ไฟล์แนบ: {name}")
            }
        })
        .collect();
    if files.len() > RELAY_MAX_FILES {
        lines.push(format!(
            "และอีก {} ไฟล์",
            files.len() - RELAY_MAX_FILES
        ));
    }
    let files = lines.join("\n");
    let mut header = format!("**{}**", ui::escape_markdown(author));
    let room = RELAY_TEXT_BUDGET.saturating_sub(header.chars().count() + files.chars().count());
    if !content.trim().is_empty() && room > 1 {
        header.push('\n');
        header.push_str(&ui::truncate(content, room - 1));
    }
    (header, files)
}

fn card(
    header: String,
    avatar: &str,
    images: Vec<(&str, bool)>,
    files: String,
    footer: String,
    jump: String,
) -> Component {
    Card::new(color::RELAY)
        .section(header, Some(avatar))
        .gallery(images)
        .text(files)
        .small(footer)
        .buttons([ui::link(jump, "ไปที่ข้อความต้นทาง")])
        .build()
}

fn render(app: &App, guild: Id<GuildMarker>, message: &Message) -> Vec<Component> {
    let is_image = |a: &&twilight_model::channel::Attachment| {
        a.content_type
            .as_deref()
            .is_some_and(|t| t.starts_with("image/"))
    };
    let attachments = crate::discord::shown_attachments(message);
    let images: Vec<(&str, bool)> = attachments
        .iter()
        .filter(is_image)
        .map(|a| (a.url.as_str(), a.filename.starts_with("SPOILER_")))
        .collect();
    let files: Vec<(&str, &str)> = attachments
        .iter()
        .filter(|a| !is_image(a))
        .map(|a| (a.filename.as_str(), a.url.as_str()))
        .collect();
    // Usernames can't impersonate others the way display names can.
    let display = app::display_name(&message.author);
    let author = if display == message.author.name {
        display.to_owned()
    } else {
        format!("{display} (@{})", message.author.name)
    };
    let (header, files) = relay_text(
        &author,
        crate::discord::shown_content(message),
        &files,
    );
    let footer = format!(
        "ส่งต่อจาก {} · {}",
        app.guild_name(guild),
        ui::ts(message.timestamp.as_micros() / 1000, 'R')
    );
    let jump = format!(
        "https://discord.com/channels/{}/{}/{}",
        guild.get(),
        message.channel_id.get(),
        message.id.get()
    );
    vec![card(
        header,
        &app::avatar_url(&message.author),
        images,
        files,
        footer,
        jump,
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_recorded_as_they_post_and_deleted_by_few_workers() {
        let relay = Relay::default();
        let (original, copy) = (Id::new(1), |n| (Id::new(n), Id::new(n)));
        assert_eq!(relay.remember(original, copy(2)), None);
        assert_eq!(relay.remember(original, copy(3)), None);
        let copies = relay.take(original, false).unwrap();
        assert_eq!(copies, [copy(2), copy(3)]);

        let deletes = &relay.deletes;
        assert_eq!(deletes.push(copies, false), (2, 0));
        let many = (10..10 + MAX_PENDING_DELETES as u64).map(copy);
        assert_eq!(deletes.push(many, false), (DELETE_WORKERS - 2, 2));
        assert_eq!(
            deletes.len(),
            (MAX_PENDING_DELETES, DELETE_WORKERS)
        );
        assert_eq!(deletes.push(vec![copy(7), copy(8)], true), (0, 0));
        let mut worker = deletes.worker();
        assert_eq!(worker.take(), Some(copy(7)));
        assert_eq!(worker.take(), Some(copy(8)));
        assert_eq!(worker.take(), Some(copy(2)));
        while worker.take().is_some() {}
        // Workers dropped early (panic, shutdown) still count themselves out.
        for _ in 1..DELETE_WORKERS {
            drop(deletes.worker());
        }
        assert_eq!(deletes.len(), (0, 0));
        assert_eq!(deletes.push(vec![copy(5)], false), (1, 0));
    }

    #[test]
    fn copies_posted_after_the_original_is_deleted_are_deleted_too() {
        let relay = Relay::default();
        let (original, copy) = (Id::new(1), |n| (Id::new(n), Id::new(n)));
        let posting = Posting::start(&relay, original).unwrap();
        assert_eq!(relay.remember(original, copy(2)), None);
        assert!(!relay.deleted(original));
        assert_eq!(relay.take(original, true), Some(vec![copy(2)]));
        assert!(relay.deleted(original));
        assert_eq!(relay.remember(original, copy(3)), Some(true));
        // A member's own delete afterwards doesn't lower a verdict's priority.
        assert_eq!(relay.take(original, false), None);
        assert_eq!(relay.remember(original, copy(4)), Some(true));
        drop(posting);
        assert!(relay.copies().posting.is_empty());
        assert!(relay.copies().posted.is_empty());
    }

    #[test]
    fn an_original_deleted_before_its_relay_starts_is_not_relayed() {
        let relay = Relay::default();
        assert_eq!(relay.take(Id::new(1), true), None);
        assert!(Posting::start(&relay, Id::new(1)).is_none());
        assert!(Posting::start(&relay, Id::new(2)).is_some());
        for id in 10..10 + MAX_GONE as u64 {
            relay.take(Id::new(id), false);
        }
        let copies = relay.copies();
        assert_eq!(copies.gone.len(), MAX_GONE);
        assert_eq!(copies.gone_order.len(), MAX_GONE);
        assert!(!copies.gone.contains(&Id::new(1)));
    }

    #[test]
    fn long_message_with_many_files_fits_discord_limits() {
        let url = format!(
            "https://cdn.discordapp.com/attachments/1/2/file.zip?{}",
            "x".repeat(220)
        );
        let names: Vec<String> = (0..12)
            .map(|i| format!("my_[file]_{i}.zip"))
            .collect();
        let files: Vec<(&str, &str)> = names
            .iter()
            .map(|n| (n.as_str(), url.as_str()))
            .collect();
        let (header, files) = relay_text(&"a".repeat(32), &"ก".repeat(5000), &files);
        assert!(files.contains("และอีก 7 ไฟล์"));
        assert!(files.contains("my(file)4.zip"));
        let card = card(
            header,
            "https://cdn.discordapp.com/embed/avatars/0.png",
            vec![("https://cdn.discordapp.com/a.png", false); 3],
            files,
            format!("ส่งต่อจาก {} · <t:1794117746:R>", "ก".repeat(100)),
            "https://discord.com/channels/1/2/3".to_owned(),
        );
        crate::discord::ui::validate_message(&[card]).unwrap();
    }
}
