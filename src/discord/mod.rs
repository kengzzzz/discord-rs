pub mod errors;
pub mod reply;
pub mod ui;

use std::{
    collections::HashMap,
    hash::{BuildHasher as _, RandomState},
    sync::{LazyLock, Mutex},
    time::Instant,
};

use twilight_model::{
    application::interaction::modal::{ModalInteractionComponent, ModalInteractionData},
    channel::{
        Attachment, Message,
        message::{Component, MessageFlags},
    },
    id::{
        Id,
        marker::{ChannelMarker, GuildMarker, UserMarker},
    },
};

use crate::app::App;

/// A forward's text lives in `message_snapshots`.
pub fn shown_content(message: &Message) -> &str {
    if !message.content.is_empty() {
        return &message.content;
    }
    message
        .message_snapshots
        .first()
        .map_or("", |s| s.message.content.as_str())
}

pub fn shown_attachments(message: &Message) -> &[Attachment] {
    if !message.attachments.is_empty() {
        return &message.attachments;
    }
    message
        .message_snapshots
        .first()
        .map_or(&[], |s| s.message.attachments.as_slice())
}

/// Drops what no handler reads before a message is queued, since senders control its size.
/// Buffers are released, not just emptied: the backlog charges what `approx_size` counts.
pub fn slim(message: &mut Message, bot: Id<UserMarker>) {
    message.mentions.retain(|m| m.id == bot);
    message.mentions.shrink_to_fit();
    // Only AI reads the replied-to message, and only when the bot is mentioned.
    if message.mentions.is_empty() {
        message.referenced_message = None;
    } else if let Some(referenced) = &mut message.referenced_message {
        slim_quoted(referenced);
    }
    strip(message);
    message.embeds = Vec::new();
    message.components = Vec::new();
}

/// Keeps only what `ai::message_text` reads.
pub fn slim_quoted(message: &mut Message) {
    message.mentions = Vec::new();
    message.referenced_message = None;
    strip(message);
    message
        .embeds
        .retain(|e| e.title.is_some() || e.description.is_some());
    message.embeds.shrink_to_fit();
    for embed in &mut message.embeds {
        embed.author = None;
        embed.fields = Vec::new();
        embed.footer = None;
        embed.image = None;
        embed.provider = None;
        embed.thumbnail = None;
        embed.url = None;
        embed.video = None;
    }
    let mut texts = Vec::new();
    text_displays(
        std::mem::take(&mut message.components),
        &mut texts,
    );
    texts.shrink_to_fit();
    message.components = texts;
}

/// The translate menu's target arrives whole, mentions and all.
pub fn slim_interaction(interaction: &mut twilight_model::application::interaction::Interaction) {
    use twilight_model::application::interaction::InteractionData;
    if let Some(InteractionData::ApplicationCommand(data)) = &mut interaction.data
        && let Some(resolved) = &mut data.resolved
    {
        resolved
            .messages
            .values_mut()
            .for_each(slim_quoted);
    }
}

fn text_displays(components: Vec<Component>, out: &mut Vec<Component>) {
    for component in components {
        match component {
            Component::TextDisplay(mut display) => {
                display.id = None;
                out.push(Component::TextDisplay(display));
            }
            Component::Container(container) => text_displays(container.components, out),
            Component::Section(section) => text_displays(section.components, out),
            _ => {}
        }
    }
}

#[allow(deprecated)] // `interaction`, still sent by Discord
fn strip(message: &mut Message) {
    message.mention_roles = Vec::new();
    message.mention_channels = Vec::new();
    message.reactions = Vec::new();
    message.sticker_items = Vec::new();
    message.activity = None;
    message.application = None;
    message.call = None;
    message.interaction = None;
    message.interaction_metadata = None;
    message.poll = None;
    message.role_subscription_data = None;
    message.thread = None;
    message.message_snapshots.truncate(1);
    message
        .message_snapshots
        .shrink_to_fit();
    for snapshot in &mut message.message_snapshots {
        let fields = &mut snapshot.message;
        fields.mentions = Vec::new();
        fields.mention_roles = Vec::new();
        fields.embeds = Vec::new();
        fields.components = Vec::new();
        fields.sticker_items = Vec::new();
        fields
            .attachments
            .iter_mut()
            .for_each(strip_attachment);
    }
    message
        .attachments
        .iter_mut()
        .for_each(strip_attachment);
}

fn strip_attachment(attachment: &mut Attachment) {
    attachment.description = None;
    attachment.title = None;
    attachment.waveform = None;
    attachment.placeholder = None;
}

/// The scan queue and relay never read the replied-to message.
pub fn without_reply(message: &Message) -> Message {
    let mut copy = message.clone();
    copy.referenced_message = None;
    copy
}

/// Approximate bytes held: JSON size plus fixed struct sizes, counted without serializing.
pub fn approx_size(message: &Message) -> usize {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    let _ = serde_json::to_writer(&mut count, message);
    count.0 + structs(message)
}

fn structs(message: &Message) -> usize {
    use std::mem::size_of;
    use twilight_model::channel::message::{Embed, Mention, MessageSnapshot};
    let snapshots: usize = message
        .message_snapshots
        .iter()
        .map(|s| {
            s.message.attachments.capacity() * size_of::<Attachment>()
                + s.message.embeds.capacity() * size_of::<Embed>()
                + components_size(&s.message.components)
        })
        .sum();
    size_of::<Message>()
        + message.attachments.capacity() * size_of::<Attachment>()
        + message.embeds.capacity() * size_of::<Embed>()
        + message.mentions.capacity() * size_of::<Mention>()
        + message.message_snapshots.capacity() * size_of::<MessageSnapshot>()
        + snapshots
        + components_size(&message.components)
        + message
            .referenced_message
            .as_deref()
            .map_or(0, structs)
}

fn components_size(components: &Vec<Component>) -> usize {
    use twilight_model::channel::message::component::SelectMenuOption;
    let nested: usize = components
        .iter()
        .map(|component| match component {
            Component::ActionRow(row) => components_size(&row.components),
            Component::Container(container) => components_size(&container.components),
            Component::Section(section) => {
                std::mem::size_of::<Component>() + components_size(&section.components)
            }
            Component::SelectMenu(menu) => menu.options.as_ref().map_or(0, |o| {
                o.capacity() * std::mem::size_of::<SelectMenuOption>()
            }),
            _ => 0,
        })
        .sum();
    components.capacity() * std::mem::size_of::<Component>() + nested
}

const BODY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Twilight's timeout stops at the headers, so a stalled body would hang the caller.
pub async fn read<T>(response: twilight_http::Response<T>) -> anyhow::Result<T>
where
    T: serde::de::DeserializeOwned + Unpin,
{
    Ok(
        tokio::time::timeout(BODY_TIMEOUT, response.model())
            .await
            .map_err(|_| anyhow::anyhow!("Discord response body timed out"))??,
    )
}

/// Nonces of card posts with an unknown outcome. Keyed by channel, so an old nonce never returns
/// a post from a previous channel.
type PendingPosts = HashMap<(Id<GuildMarker>, &'static str, Id<ChannelMarker>), String>;
static PENDING_POSTS: LazyLock<Mutex<PendingPosts>> = LazyLock::new(Mutex::default);

/// A timed-out post may have landed; the retry reuses the nonce so Discord returns it instead of
/// posting a duplicate.
pub async fn post_card(
    app: &App,
    guild: Id<GuildMarker>,
    purpose: &'static str,
    channel: Id<ChannelMarker>,
    components: &[Component],
) -> anyhow::Result<Message> {
    let key = (guild, purpose, channel);
    let nonce = PENDING_POSTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(key)
        .or_insert_with(|| {
            format!(
                "c{:016x}",
                RandomState::new().hash_one((guild, purpose, Instant::now()))
            )
        })
        .clone();
    let body = card_payload(components, &nonce)?;
    let forget = || {
        PENDING_POSTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&key);
    };
    let response = match app
        .http
        .create_message(channel)
        .payload_json(&body)
        .await
    {
        Ok(response) => response,
        Err(error) => {
            if errors::refused(&error) {
                forget();
            }
            return Err(error.into());
        }
    };
    // The ID is unknown until read back, so a failure here still reuses the nonce.
    let message = read(response).await?;
    forget();
    anyhow::ensure!(
        message.channel_id == channel,
        "Discord returned a message from another channel"
    );
    Ok(message)
}

fn card_payload(components: &[Component], nonce: &str) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&serde_json::json!({
        "flags": MessageFlags::IS_COMPONENTS_V2.bits(),
        "components": components,
        "allowed_mentions": { "parse": [] },
        "nonce": nonce,
        "enforce_nonce": true,
    }))
}

pub async fn dm_or_channel(
    app: &App,
    user: Id<UserMarker>,
    fallback: Option<Id<ChannelMarker>>,
    components: &[Component],
    mention_in_fallback: bool,
) -> bool {
    dm_or_post(
        app,
        user,
        fallback,
        components,
        components,
        mention_in_fallback,
    )
    .await
}

/// `dm` goes by DM; `components` goes to the channel if the DM fails.
pub async fn dm_or_post(
    app: &App,
    user: Id<UserMarker>,
    fallback: Option<Id<ChannelMarker>>,
    dm: &[Component],
    components: &[Component],
    mention_in_fallback: bool,
) -> bool {
    if let Ok(response) = app
        .http
        .create_private_channel(user)
        .await
        && let Ok(channel) = read(response).await
        && app
            .http
            .create_message(channel.id)
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(dm)
            .await
            .is_ok()
    {
        return true;
    }
    let Some(fallback) = fallback else {
        return false;
    };
    let mut parts = Vec::with_capacity(components.len() + 1);
    if mention_in_fallback {
        parts.push(Component::TextDisplay(
            twilight_model::channel::message::component::TextDisplay {
                id: None,
                content: format!("<@{user}>"),
            },
        ));
    }
    parts.extend_from_slice(components);
    let allowed = twilight_model::channel::message::AllowedMentions {
        users: vec![user],
        ..Default::default()
    };
    match app
        .http
        .create_message(fallback)
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(&parts)
        .allowed_mentions(Some(&allowed))
        .await
    {
        Ok(_) => true,
        Err(error) => {
            tracing::warn!(channel = fallback.get(), %error, "fallback message failed");
            false
        }
    }
}

pub fn modal_value<'a>(data: &'a ModalInteractionData, custom_id: &str) -> Option<&'a str> {
    data.components
        .iter()
        .find_map(|component| find_text(component, custom_id))
}

fn find_text<'a>(component: &'a ModalInteractionComponent, custom_id: &str) -> Option<&'a str> {
    match component {
        ModalInteractionComponent::Label(label) => find_text(&label.component, custom_id),
        ModalInteractionComponent::ActionRow(row) => row
            .components
            .iter()
            .find_map(|c| find_text(c, custom_id)),
        ModalInteractionComponent::TextInput(input) if input.custom_id == custom_id => {
            Some(input.value.as_str())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forward_shows_its_snapshot_content_and_files() {
        let message = |content: &str, snapshot: serde_json::Value| -> Message {
            serde_json::from_value(serde_json::json!({
                "id": "10", "channel_id": "20", "guild_id": "30",
                "author": {"id": "40", "username": "a", "discriminator": "0", "avatar": null},
                "content": content, "timestamp": "2026-11-08T00:00:00+00:00",
                "edited_timestamp": null, "tts": false, "mention_everyone": false,
                "mentions": [], "mention_roles": [], "attachments": [], "embeds": [],
                "pinned": false, "type": 0, "flags": 16384,
                "message_reference": {"type": 1, "channel_id": "21", "message_id": "11"},
                "message_snapshots": [{"message": snapshot}]
            }))
            .unwrap()
        };
        let snapshot = serde_json::json!({
            "content": "free nitro discord.gg/x", "type": 0,
            "timestamp": "2026-11-07T00:00:00+00:00", "edited_timestamp": null,
            "flags": 0, "mentions": [], "mention_roles": [], "embeds": [], "components": [],
            "attachments": [{
                "id": "50", "filename": "scam.png", "size": 10, "url": "https://cdn.discordapp.com/x.png",
                "proxy_url": "https://media.discordapp.net/x.png", "content_type": "image/png"
            }]
        });
        let forward = message("", snapshot.clone());
        assert_eq!(shown_content(&forward), "free nitro discord.gg/x");
        assert_eq!(
            shown_attachments(&forward)[0].filename,
            "scam.png"
        );
        let own = message("hello", snapshot);
        assert_eq!(shown_content(&own), "hello");
    }

    #[test]
    fn card_payload_is_a_components_v2_message_with_a_nonce() {
        let card = ui::Card::new(ui::color::BRAND)
            .title("t")
            .build();
        let body: serde_json::Value =
            serde_json::from_slice(&card_payload(&[card], "c0123456789abcdef").unwrap()).unwrap();
        assert_eq!(
            body["flags"],
            MessageFlags::IS_COMPONENTS_V2.bits()
        );
        assert_eq!(body["nonce"], "c0123456789abcdef");
        assert_eq!(body["enforce_nonce"], true);
        assert_eq!(
            body["allowed_mentions"]["parse"],
            serde_json::json!([])
        );
        assert_eq!(body["components"][0]["type"], 17);
    }

    #[test]
    fn slim_interaction_trims_the_translate_target() {
        use twilight_model::application::interaction::{Interaction, InteractionData};
        let user = |id: &str| serde_json::json!({"id": id, "username": "u", "discriminator": "0", "avatar": null, "public_flags": 0});
        let mentions: Vec<_> = (100..195)
            .map(|id| user(&id.to_string()))
            .collect();
        let mut interaction: Interaction = serde_json::from_value(serde_json::json!({
            "id": "900", "application_id": "1", "type": 2, "token": "t", "version": 1,
            "guild_id": "30", "channel_id": "20", "attachment_size_limit": 10485760,
            "entitlements": [], "authorizing_integration_owners": {"0": "30"}, "context": 0,
            "member": {"user": user("7"), "roles": [], "joined_at": "2026-01-01T00:00:00+00:00",
                "deaf": false, "mute": false, "flags": 0, "permissions": "2048"},
            "data": {"id": "77", "name": "Translate to Thai", "type": 3, "target_id": "555",
                "resolved": {"messages": {"555": {
                    "id": "555", "channel_id": "20", "guild_id": "30", "author": user("8"),
                    "content": "hello", "timestamp": "2026-11-08T00:00:00+00:00",
                    "edited_timestamp": null, "tts": false, "mention_everyone": false,
                    "mentions": mentions, "mention_roles": [], "attachments": [],
                    "embeds": [{"type": "image", "url": "https://example.com/i.png"},
                        {"type": "rich", "description": "d"}],
                    "pinned": false, "type": 0
                }}}}
        }))
        .unwrap();
        slim_interaction(&mut interaction);
        let Some(InteractionData::ApplicationCommand(data)) = &interaction.data else {
            panic!("command data");
        };
        let target = &data.resolved.as_ref().unwrap().messages[&Id::new(555)];
        assert_eq!(target.content, "hello");
        assert_eq!(target.mentions.capacity(), 0);
        assert_eq!(target.embeds.len(), 1);
        assert_eq!(target.embeds[0].description.as_deref(), Some("d"));
    }

    #[test]
    fn slim_keeps_what_handlers_read() {
        let user = |id: &str| serde_json::json!({"id": id, "username": "u", "discriminator": "0", "avatar": null, "public_flags": 0});
        let file = serde_json::json!({
            "id": "50", "filename": "a.png", "size": 10, "url": "https://cdn.discordapp.com/a.png",
            "proxy_url": "https://media.discordapp.net/a.png", "content_type": "image/png",
            "description": "x".repeat(1024)
        });
        let message = |mentions: serde_json::Value| -> Message {
            let base = |mentions: serde_json::Value| {
                serde_json::json!({
                    "id": "10", "channel_id": "20", "guild_id": "30", "author": user("40"),
                    "content": "hi", "timestamp": "2026-11-08T00:00:00+00:00",
                    "edited_timestamp": null, "tts": false, "mention_everyone": false,
                    "mentions": mentions, "mention_roles": ["60"], "attachments": [file.clone()],
                    "embeds": [{"type": "rich", "title": "t", "description": "d",
                        "image": {"url": format!("https://example.com/{}", "u".repeat(2000))}},
                        {"type": "image", "url": "https://example.com/i.png"}],
                    "components": [{"type": 17, "components": [
                        {"type": 10, "content": "card"},
                        {"type": 9, "components": [{"type": 10, "content": "side"}],
                            "accessory": {"type": 2, "style": 1, "label": "b", "custom_id": "c"}},
                        {"type": 1, "components": [{"type": 2, "style": 1, "label": "b", "custom_id": "d"}]}
                    ]}],
                    "pinned": false, "type": 19
                })
            };
            let mut outer = base(mentions);
            outer["referenced_message"] = base(serde_json::json!([user("41"), user("42")]));
            serde_json::from_value(outer).unwrap()
        };
        let bot = Id::new(99);

        let mut plain = message(serde_json::json!([user("41"), user("42")]));
        let before = approx_size(&plain);
        slim(&mut plain, bot);
        assert!(plain.mentions.is_empty() && plain.mention_roles.is_empty());
        assert!(plain.referenced_message.is_none());
        assert_eq!(plain.attachments[0].description, None);
        assert_eq!(
            plain.attachments[0].url,
            "https://cdn.discordapp.com/a.png"
        );
        assert!(approx_size(&plain) * 3 < before);
        assert!(plain.embeds.is_empty());
        assert_eq!(plain.mentions.capacity(), 0);
        assert_eq!(plain.mention_roles.capacity(), 0);

        let mut asked = message(serde_json::json!([user("41"), user("99")]));
        slim(&mut asked, bot);
        assert_eq!(asked.mentions.len(), 1);
        assert_eq!(asked.mentions.capacity(), 1);
        let referenced = asked
            .referenced_message
            .as_deref()
            .unwrap();
        assert_eq!(referenced.mentions.capacity(), 0);
        assert_eq!(referenced.content, "hi");
        assert_eq!(referenced.embeds[0].title.as_deref(), Some("t"));
        assert_eq!(
            referenced.embeds[0]
                .description
                .as_deref(),
            Some("d")
        );
        assert!(referenced.embeds[0].image.is_none());
        assert_eq!(referenced.embeds.len(), 1);
        let texts: Vec<_> = referenced
            .components
            .iter()
            .map(|c| match c {
                Component::TextDisplay(d) => d.content.as_str(),
                _ => panic!("only text is kept"),
            })
            .collect();
        assert_eq!(texts, ["card", "side"]);
        assert!(
            approx_size(referenced)
                >= std::mem::size_of::<Message>()
                    + std::mem::size_of::<twilight_model::channel::message::Embed>()
                    + 2 * std::mem::size_of::<Component>()
        );
        assert_eq!(referenced.attachments[0].description, None);

        let mut forward: Message = serde_json::from_value(serde_json::json!({
            "id": "12", "channel_id": "20", "guild_id": "30", "author": user("40"),
            "content": "", "timestamp": "2026-11-08T00:00:00+00:00", "edited_timestamp": null,
            "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
            "attachments": [], "embeds": [], "pinned": false, "type": 0, "flags": 16384,
            "message_snapshots": [{"message": {
                "content": "fwd", "type": 0, "timestamp": "2026-11-07T00:00:00+00:00",
                "edited_timestamp": null, "flags": 0, "mentions": [user("41")], "mention_roles": ["60"],
                "embeds": [{"type": "rich", "title": "t", "url": "https://example.com/x"}],
                "components": [], "attachments": [file.clone()]
            }}]
        }))
        .unwrap();
        slim(&mut forward, bot);
        let snapshot = &forward.message_snapshots[0].message;
        assert_eq!(snapshot.content, "fwd");
        assert_eq!(snapshot.embeds.capacity(), 0);
        assert_eq!(snapshot.mentions.capacity(), 0);
        assert_eq!(snapshot.attachments[0].description, None);
        assert_eq!(shown_attachments(&forward).len(), 1);
    }
}
