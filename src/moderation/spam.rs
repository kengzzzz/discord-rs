use std::{
    collections::{HashSet, VecDeque},
    hash::{DefaultHasher, Hash, Hasher},
    num::NonZeroU64,
    sync::Mutex,
    time::{Duration, Instant},
};

use twilight_model::{
    channel::Attachment,
    id::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
};

pub const WINDOW: Duration = Duration::from_secs(10 * 60);
pub const CHANNEL_THRESHOLD: usize = 4;
const MAX_ENTRIES_PER_USER: usize = 64;
/// Shared by all members, so many accounts can't grow it without bound.
const MAX_ENTRIES: usize = 8192;
const MIN_CAMPAIGN_TOKENS: usize = 3;
const MIN_IDENTICAL_CHARS: usize = 15;

pub type MessageRef = (Id<ChannelMarker>, Id<MessageMarker>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpamKind {
    Identical,
    Campaign,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    Spam { kind: SpamKind, messages: Vec<MessageRef> },
}

pub struct Observed<'a> {
    pub guild: Id<GuildMarker>,
    pub user: Id<UserMarker>,
    pub channel: Id<ChannelMarker>,
    pub message: Id<MessageMarker>,
    pub content: &'a str,
    pub attachments: &'a [Attachment],
}

struct Entry {
    at: Instant,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    message: MessageRef,
    exact: Option<NonZeroU64>,
    campaign: Option<NonZeroU64>,
}

impl Entry {
    fn of(&self, guild: Id<GuildMarker>, user: Id<UserMarker>) -> bool {
        self.guild == guild && self.user == user
    }
}

#[derive(Default)]
struct Windows {
    entries: VecDeque<Entry>,
    full: bool,
}

#[derive(Default)]
pub struct SpamDetector {
    windows: Mutex<Windows>,
}

impl SpamDetector {
    fn windows(&self) -> std::sync::MutexGuard<'_, Windows> {
        self.windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn observe(&self, msg: &Observed<'_>, now: Instant) -> Verdict {
        let tokens = normalize_tokens(msg.content);
        let exact = exact_fingerprint(msg.content, &tokens, msg.attachments);
        let campaign = campaign_fingerprint(&tokens, msg.attachments);
        if exact.is_none() && campaign.is_none() {
            return Verdict::Clean;
        }

        let mut windows = self.windows();
        windows.forget_expired(now);
        let (guild, user) = (msg.guild, msg.user);
        if windows
            .entries
            .iter()
            .filter(|e| e.of(guild, user))
            .count()
            >= MAX_ENTRIES_PER_USER
            && let Some(index) = windows
                .entries
                .iter()
                .position(|e| e.of(guild, user))
        {
            windows.entries.remove(index);
        }
        if windows.entries.len() >= MAX_ENTRIES {
            windows.entries.pop_front();
            metrics::counter!("spam_entries_evicted_total").increment(1);
            if !windows.full {
                windows.full = true;
                tracing::warn!(
                    "spam check is forgetting messages early: over {MAX_ENTRIES} in 10 minutes"
                );
            }
        }
        windows.entries.push_back(Entry {
            at: now,
            guild,
            user,
            message: (msg.channel, msg.message),
            exact: exact.map(nonzero),
            campaign: campaign.map(nonzero),
        });

        let verdict = [(SpamKind::Identical, exact), (SpamKind::Campaign, campaign)]
            .into_iter()
            .find_map(|(kind, print)| {
                let print = nonzero(print?);
                let matching: Vec<MessageRef> = windows
                    .entries
                    .iter()
                    .filter(|e| e.of(guild, user))
                    .filter(|e| match kind {
                        SpamKind::Identical => e.exact == Some(print),
                        SpamKind::Campaign => e.campaign == Some(print),
                    })
                    .map(|e| e.message)
                    .collect();
                let channels: HashSet<_> = matching
                    .iter()
                    .map(|(channel, _)| *channel)
                    .collect();
                (channels.len() >= CHANNEL_THRESHOLD)
                    .then_some(Verdict::Spam { kind, messages: matching })
            });

        match verdict {
            Some(spam) => {
                windows.forget(guild, user);
                spam
            }
            None => Verdict::Clean,
        }
    }

    pub fn forget(&self, guild: Id<GuildMarker>, user: Id<UserMarker>) {
        self.windows().forget(guild, user);
    }

    pub fn sweep(&self, now: Instant) {
        let mut windows = self.windows();
        windows.forget_expired(now);
        if windows.entries.capacity() > 4 * windows.entries.len().max(64) {
            windows.entries.shrink_to_fit();
        }
    }

    pub fn remembered(&self) -> usize {
        self.windows().entries.len()
    }
}

impl Windows {
    fn forget_expired(&mut self, now: Instant) {
        while self
            .entries
            .front()
            .is_some_and(|e| now.saturating_duration_since(e.at) > WINDOW)
        {
            self.entries.pop_front();
        }
        if self.entries.len() < MAX_ENTRIES / 2 {
            self.full = false;
        }
    }

    fn forget(&mut self, guild: Id<GuildMarker>, user: Id<UserMarker>) {
        self.entries
            .retain(|e| !e.of(guild, user));
    }
}

fn nonzero(print: u64) -> NonZeroU64 {
    NonZeroU64::new(print).unwrap_or(NonZeroU64::MIN)
}

fn exact_fingerprint(content: &str, tokens: &[String], attachments: &[Attachment]) -> Option<u64> {
    let content = content.trim();
    if content.is_empty() && attachments.is_empty() {
        return None;
    }
    // Short replies like "555" are normal in several threads; without a payload only longer
    // text counts.
    let payload = !attachments.is_empty()
        || tokens
            .iter()
            .any(|t| t == MENTION || t.starts_with("url:"));
    let words = tokens
        .iter()
        .filter(|t| !is_placeholder(t));
    if !payload
        && words.clone().count() < MIN_CAMPAIGN_TOKENS
        && words
            .map(|t| t.chars().count())
            .sum::<usize>()
            < MIN_IDENTICAL_CHARS
    {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let mut prints: Vec<(u64, u64, u64, &str)> = attachments
        .iter()
        .map(|a| {
            (
                a.size,
                a.width.unwrap_or_default(),
                a.height.unwrap_or_default(),
                a.content_type
                    .as_deref()
                    .unwrap_or_default(),
            )
        })
        .collect();
    prints.sort_unstable();
    prints.hash(&mut hasher);
    Some(hasher.finish())
}

const MENTION: &str = "@mention";
const EMOJI: &str = ":emoji:";

fn is_placeholder(token: &str) -> bool {
    token == MENTION || token == EMOJI
}

fn campaign_fingerprint(tokens: &[String], attachments: &[Attachment]) -> Option<u64> {
    // Links collapse to their host, so bare links alone would look like one campaign. Exact
    // duplicates are still caught.
    if tokens
        .iter()
        .filter(|t| !is_placeholder(t))
        .count()
        < MIN_CAMPAIGN_TOKENS
    {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    tokens.hash(&mut hasher);
    attachment_shape(attachments).hash(&mut hasher);
    Some(hasher.finish())
}

fn attachment_shape(attachments: &[Attachment]) -> [u8; 4] {
    let mut shape = [0u8; 4];
    for attachment in attachments {
        let kind = attachment
            .content_type
            .as_deref()
            .unwrap_or_default();
        let slot = if kind.starts_with("image/") || attachment.width.is_some() {
            0
        } else if kind.starts_with("video/") {
            1
        } else if kind.starts_with("audio/") || attachment.duration_secs.is_some() {
            2
        } else {
            3
        };
        shape[slot] = shape[slot].saturating_add(1);
    }
    shape
}

pub fn normalize_tokens(content: &str) -> Vec<String> {
    content
        .split_whitespace()
        .filter_map(|raw| {
            let trimmed = raw.trim_matches(|c: char| {
                c.is_ascii()
                    && !c.is_ascii_alphanumeric()
                    && !matches!(c, '<' | '>' | '@' | '#' | ':' | '/' | '.')
            });
            if trimmed.is_empty() {
                return None;
            }
            let lower = trimmed.to_lowercase();
            if is_mention(&lower) {
                return Some(MENTION.to_owned());
            }
            if (lower.starts_with("<:") || lower.starts_with("<a:")) && lower.ends_with('>') {
                return Some(EMOJI.to_owned());
            }
            if let Some(host) = link_host(&lower) {
                return Some(format!("url:{host}"));
            }
            Some(mask_digit_runs(&lower))
        })
        .collect()
}

fn is_mention(token: &str) -> bool {
    (token.starts_with("<@") || token.starts_with("<#")) && token.ends_with('>')
}

fn link_host(token: &str) -> Option<String> {
    if let Some(rest) = token
        .strip_prefix("https://")
        .or_else(|| token.strip_prefix("http://"))
    {
        let host = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        return (!host.is_empty()).then(|| {
            host.trim_start_matches("www.")
                .to_owned()
        });
    }
    if token.starts_with("discord.gg/") || token.starts_with("discord.com/invite/") {
        return Some("discord.gg".to_owned());
    }
    let candidate = token
        .trim_start_matches("www.")
        .split('/')
        .next()
        .unwrap_or_default();
    let candidate = candidate.trim_end_matches('.');
    let tld = candidate
        .rsplit_once('.')
        .map(|(_, tld)| tld)?;
    let looks_like_domain = !candidate.contains('@')
        && tld.len() >= 2
        && tld
            .chars()
            .all(|c| c.is_ascii_alphabetic())
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    looks_like_domain.then(|| candidate.to_owned())
}

fn mask_digit_runs(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    let mut run = 0usize;
    let mut run_start = 0usize;
    for (index, ch) in token.char_indices() {
        if ch.is_ascii_digit() {
            if run == 0 {
                run_start = index;
            }
            run += 1;
            continue;
        }
        flush_run(&mut out, &token[run_start..index], run);
        run = 0;
        out.push(ch);
    }
    flush_run(&mut out, &token[run_start..], run);
    out
}

fn flush_run(out: &mut String, digits: &str, run: usize) {
    match run {
        0 => {}
        1 | 2 => out.push_str(digits),
        _ => out.push_str("{num}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(size: u64) -> Attachment {
        serde_json::from_value(serde_json::json!({
            "id": "1",
            "filename": "a.png",
            "size": size,
            "url": "https://cdn.discordapp.com/a.png",
            "proxy_url": "https://media.discordapp.net/a.png",
            "content_type": "image/png",
            "width": 100,
            "height": 100
        }))
        .unwrap()
    }

    fn observe(
        detector: &SpamDetector,
        channel: u64,
        message: u64,
        content: &str,
        attachments: &[Attachment],
        at: Instant,
    ) -> Verdict {
        detector.observe(
            &Observed {
                guild: Id::new(1),
                user: Id::new(2),
                channel: Id::new(channel),
                message: Id::new(message),
                content,
                attachments,
            },
            at,
        )
    }

    #[test]
    fn identical_message_in_four_channels_returns_every_copy() {
        let d = SpamDetector::default();
        let t = Instant::now();
        assert_eq!(
            observe(&d, 10, 1, "free nitro for everyone", &[], t),
            Verdict::Clean
        );
        assert_eq!(
            observe(&d, 10, 2, "free nitro for everyone", &[], t),
            Verdict::Clean
        );
        assert_eq!(
            observe(&d, 11, 3, "free nitro for everyone", &[], t),
            Verdict::Clean
        );
        assert_eq!(
            observe(&d, 12, 4, "free nitro for everyone", &[], t),
            Verdict::Clean
        );
        let Verdict::Spam { kind, messages } =
            observe(&d, 13, 5, "free nitro for everyone", &[], t)
        else {
            panic!("expected spam");
        };
        assert_eq!(kind, SpamKind::Identical);
        assert_eq!(messages.len(), 5);
        assert_eq!(
            observe(&d, 14, 6, "free nitro for everyone", &[], t),
            Verdict::Clean
        );
    }

    #[test]
    fn short_replies_in_many_threads_are_not_spam() {
        let d = SpamDetector::default();
        let t = Instant::now();
        for text in ["555", "ขอบคุณครับ", "gg ez"] {
            for channel in 10..16 {
                assert_eq!(
                    observe(&d, channel, channel, text, &[], t),
                    Verdict::Clean,
                    "{text}"
                );
            }
        }
        for channel in 20..23 {
            observe(&d, channel, channel, "<@99> 555", &[], t);
        }
        assert!(matches!(
            observe(&d, 23, 23, "<@99> 555", &[], t),
            Verdict::Spam { kind: SpamKind::Identical, .. }
        ));
    }

    #[test]
    fn emoji_only_and_mention_greetings_are_not_spam() {
        let d = SpamDetector::default();
        let t = Instant::now();
        for channel in 10..16 {
            assert_eq!(
                observe(
                    &d,
                    channel,
                    channel,
                    "<:pepelaugh:891234567890123456>",
                    &[],
                    t
                ),
                Verdict::Clean
            );
        }
        for (i, channel) in (20..26).enumerate() {
            let text = format!("<@{}> thanks bro", 11 + i);
            assert_eq!(
                observe(&d, channel, channel + 100, &text, &[], t),
                Verdict::Clean,
                "{text}"
            );
        }
    }

    #[test]
    fn copies_outside_the_window_do_not_count() {
        let d = SpamDetector::default();
        let t = Instant::now();
        for (i, channel) in [10, 11, 12].into_iter().enumerate() {
            observe(
                &d,
                channel,
                i as u64 + 1,
                "same text here",
                &[],
                t,
            );
        }
        let later = t + WINDOW + Duration::from_secs(1);
        assert_eq!(
            observe(&d, 13, 9, "same text here", &[], later),
            Verdict::Clean
        );
    }

    #[test]
    fn campaign_variants_with_numbers_and_links_match() {
        let d = SpamDetector::default();
        let t = Instant::now();
        let texts = [
            "Claim your $5000 gift at https://scam.example/a1",
            "claim your $1234 gift at https://scam.example/b2",
            "CLAIM your $9999 gift at http://www.scam.example/zz",
            "Claim your $4242 gift at https://scam.example/",
        ];
        let mut last = Verdict::Clean;
        for (i, text) in texts.iter().enumerate() {
            last = observe(&d, 20 + i as u64, i as u64 + 1, text, &[], t);
        }
        assert!(matches!(
            last,
            Verdict::Spam { kind: SpamKind::Campaign, .. }
        ));
    }

    #[test]
    fn different_thai_messages_with_same_image_shape_do_not_collide() {
        let d = SpamDetector::default();
        let t = Instant::now();
        let texts =
            ["สวัสดีครับ ทุกคน วันนี้", "ใครว่างไปฟาร์ม กันไหม", "ขอบคุณที่ช่วยนะ เพื่อน", "เย็นนี้ล่า Eidolon กัน"];
        for (i, text) in texts.iter().enumerate() {
            let att = [attachment(1000 + i as u64)];
            assert_eq!(
                observe(&d, 30 + i as u64, i as u64 + 1, text, &att, t),
                Verdict::Clean
            );
        }
    }

    #[test]
    fn different_images_without_text_are_not_a_campaign() {
        let d = SpamDetector::default();
        let t = Instant::now();
        for i in 0..5u64 {
            let att = [attachment(5000 + i)];
            assert_eq!(
                observe(&d, 40 + i, i + 1, "", &att, t),
                Verdict::Clean
            );
        }
    }

    #[test]
    fn windows_are_capped_across_members_and_per_member() {
        let d = SpamDetector::default();
        let t = Instant::now();
        let post = |user: u64, message: u64, content: &str| {
            d.observe(
                &Observed {
                    guild: Id::new(1),
                    user: Id::new(user),
                    channel: Id::new(10 + message % 4),
                    message: Id::new(message),
                    content,
                    attachments: &[],
                },
                t,
            )
        };
        for i in 0..MAX_ENTRIES_PER_USER as u64 + 10 {
            post(
                1,
                i + 1,
                &format!("ordinary message number {i} with words"),
            );
        }
        assert_eq!(d.remembered(), MAX_ENTRIES_PER_USER);
        for user in 0..MAX_ENTRIES as u64 {
            post(
                100 + user,
                1000 + user,
                "hello everyone how are you today",
            );
        }
        assert_eq!(d.remembered(), MAX_ENTRIES);
        // A member's own window still works at the cap.
        let spam = "join my server for free nitro today";
        let verdicts: Vec<_> = (0..4)
            .map(|i| post(5, 100_000 + i, spam))
            .collect();
        assert!(matches!(verdicts[3], Verdict::Spam { ref messages, .. } if messages.len() == 4));
        // Each post at the cap evicted the oldest entry.
        assert_eq!(d.remembered(), MAX_ENTRIES - 4);
        d.forget(Id::new(1), Id::new(200));
        assert_eq!(d.remembered(), MAX_ENTRIES - 5);
        d.sweep(t + WINDOW + Duration::from_secs(1));
        assert_eq!(d.remembered(), 0);
    }

    #[test]
    fn identical_images_without_text_are_spam() {
        let d = SpamDetector::default();
        let t = Instant::now();
        let att = [attachment(777)];
        let mut last = Verdict::Clean;
        for i in 0..4u64 {
            last = observe(&d, 50 + i, i + 1, "", &att, t);
        }
        assert!(matches!(
            last,
            Verdict::Spam { kind: SpamKind::Identical, .. }
        ));
    }

    #[test]
    fn tokens_keep_thai_and_mask_mentions_links_and_numbers() {
        assert_eq!(
            normalize_tokens("<@123> ดูนี่!! www.Example.com/x โค้ด 123456 ok."),
            ["@mention", "ดูนี่", "url:example.com", "โค้ด", "{num}", "ok."]
        );
        assert_eq!(mask_digit_runs("a12b3456"), "a12b{num}");
        assert_eq!(
            link_host("discord.gg/abc").as_deref(),
            Some("discord.gg")
        );
        assert_eq!(link_host("1.5"), None);
    }

    #[test]
    fn different_bare_links_to_one_site_are_not_a_campaign() {
        let d = SpamDetector::default();
        let t = Instant::now();
        for (i, page) in ["Hate", "Soma", "Saryn", "Wisp"]
            .iter()
            .enumerate()
        {
            let text = format!("https://wiki.warframe.com/w/{page}");
            assert_eq!(
                observe(&d, 50 + i as u64, i as u64 + 1, &text, &[], t),
                Verdict::Clean
            );
        }
    }

    #[test]
    fn short_campaign_with_link_still_matches() {
        let d = SpamDetector::default();
        let t = Instant::now();
        let mut last = Verdict::Clean;
        for i in 0..4u64 {
            let text = format!("free nitro https://gift.example/{i}");
            last = observe(&d, 60 + i, i + 1, &text, &[], t);
        }
        assert!(matches!(
            last,
            Verdict::Spam { kind: SpamKind::Campaign, .. }
        ));
    }
}
