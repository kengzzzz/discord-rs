use twilight_model::channel::message::{
    Component,
    component::{
        ButtonStyle, MediaGallery, MediaGalleryItem, SeparatorSpacingSize, UnfurledMediaItem,
    },
};
use twilight_util::builder::message::{
    ActionRowBuilder, ButtonBuilder, ContainerBuilder, SectionBuilder, SeparatorBuilder,
    TextDisplayBuilder, ThumbnailBuilder,
};

pub mod color {
    pub const BRAND: u32 = 0xF1C40F;
    pub const INFO: u32 = 0x5865F2;
    pub const SUCCESS: u32 = 0x2ECC71;
    pub const WARNING: u32 = 0xE67E22;
    pub const DANGER: u32 = 0xE74C3C;
    pub const AI: u32 = 0x8E7CF0;
    pub const RELAY: u32 = 0x6495ED;
    pub const MUTED: u32 = 0x95A5A6;
}

pub const TEXT_BLOCK_MAX: usize = 1900;
pub const MESSAGE_TEXT_BUDGET: usize = 3800;

pub struct Card {
    inner: ContainerBuilder,
    text_len: usize,
}

impl Card {
    pub fn new(accent: u32) -> Self {
        Self { inner: ContainerBuilder::new().accent_color(Some(accent)), text_len: 0 }
    }

    pub fn title(self, title: impl AsRef<str>) -> Self {
        self.text(format!("### {}", title.as_ref()))
    }

    pub fn text(mut self, markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        if markdown.trim().is_empty() {
            return self;
        }
        for chunk in split_text(&markdown, TEXT_BLOCK_MAX) {
            self.text_len += chunk.chars().count();
            self.inner = self
                .inner
                .component(TextDisplayBuilder::new(chunk).build());
        }
        self
    }

    pub fn small(self, markdown: impl AsRef<str>) -> Self {
        let mut out = String::new();
        for line in markdown
            .as_ref()
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str("-# ");
            out.push_str(line);
        }
        self.text(out)
    }

    pub fn section(mut self, markdown: impl Into<String>, thumbnail_url: Option<&str>) -> Self {
        let markdown = markdown.into();
        let Some(url) = thumbnail_url.filter(|u| u.starts_with("https://")) else {
            return self.text(markdown);
        };
        let thumb = ThumbnailBuilder::new(media(url)).build();
        let mut section = SectionBuilder::new(thumb);
        for chunk in split_text(&markdown, TEXT_BLOCK_MAX)
            .into_iter()
            .take(3)
        {
            self.text_len += chunk.chars().count();
            section = section.component(TextDisplayBuilder::new(chunk).build());
        }
        self.inner = self.inner.component(section.build());
        self
    }

    pub fn section_button(mut self, markdown: impl Into<String>, button: Component) -> Self {
        let markdown = markdown.into();
        self.text_len += markdown.chars().count();
        let section =
            SectionBuilder::new(button).component(TextDisplayBuilder::new(markdown).build());
        self.inner = self.inner.component(section.build());
        self
    }

    pub fn gallery<'a>(mut self, images: impl IntoIterator<Item = (&'a str, bool)>) -> Self {
        let items: Vec<MediaGalleryItem> = images
            .into_iter()
            .filter(|(url, _)| url.starts_with("https://"))
            .take(10)
            .map(|(url, spoiler)| MediaGalleryItem {
                description: None,
                media: media(url),
                spoiler: spoiler.then_some(true),
            })
            .collect();
        if !items.is_empty() {
            self.inner = self
                .inner
                .component(Component::MediaGallery(MediaGallery {
                    id: None,
                    items,
                }));
        }
        self
    }

    pub fn raw(mut self, component: Component) -> Self {
        self.inner = self.inner.component(component);
        self
    }

    pub fn divider(mut self) -> Self {
        self.inner = self.inner.component(
            SeparatorBuilder::new()
                .divider(true)
                .spacing(SeparatorSpacingSize::Small)
                .build(),
        );
        self
    }

    pub fn gap(mut self) -> Self {
        self.inner = self.inner.component(
            SeparatorBuilder::new()
                .divider(false)
                .spacing(SeparatorSpacingSize::Small)
                .build(),
        );
        self
    }

    pub fn buttons(mut self, buttons: impl IntoIterator<Item = Component>) -> Self {
        let mut row = ActionRowBuilder::new();
        let mut any = false;
        for button in buttons.into_iter().take(5) {
            row = row.component(button);
            any = true;
        }
        if any {
            self.inner = self.inner.component(row.build());
        }
        self
    }

    pub fn text_len(&self) -> usize {
        self.text_len
    }

    pub fn build(self) -> Component {
        self.inner.build().into()
    }
}

pub fn media(url: &str) -> UnfurledMediaItem {
    UnfurledMediaItem {
        url: url.to_owned(),
        proxy_url: None,
        height: None,
        width: None,
        content_type: None,
    }
}

pub fn button(
    style: ButtonStyle,
    custom_id: impl Into<String>,
    label: impl Into<String>,
) -> Component {
    ButtonBuilder::new(style)
        .custom_id(custom_id)
        .label(label)
        .build()
        .into()
}

pub fn button_with(
    style: ButtonStyle,
    custom_id: impl Into<String>,
    label: impl Into<String>,
    disabled: bool,
) -> Component {
    ButtonBuilder::new(style)
        .custom_id(custom_id)
        .label(label)
        .disabled(disabled)
        .build()
        .into()
}

pub fn link(url: impl Into<String>, label: impl Into<String>) -> Component {
    ButtonBuilder::new(ButtonStyle::Link)
        .url(url)
        .label(label)
        .build()
        .into()
}

pub fn notice(accent: u32, title: &str, body: &str) -> Vec<Component> {
    vec![
        Card::new(accent)
            .title(title)
            .text(body)
            .build(),
    ]
}

pub fn error(body: &str) -> Vec<Component> {
    notice(color::DANGER, "เกิดข้อผิดพลาด", body)
}

pub fn success(title: &str, body: &str) -> Vec<Component> {
    notice(color::SUCCESS, title, body)
}

pub fn split_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        if rest.chars().count() <= max_chars {
            chunks.push(rest.to_owned());
            break;
        }
        let limit = rest
            .char_indices()
            .nth(max_chars)
            .map_or(rest.len(), |(i, _)| i);
        let window = &rest[..limit];
        let cut = window
            .rfind("\n\n")
            .or_else(|| window.rfind('\n'))
            .or_else(|| window.rfind(char::is_whitespace))
            .filter(|&i| i > limit / 2)
            .unwrap_or(limit);
        chunks.push(rest[..cut].trim_end().to_owned());
        rest = rest[cut..].trim_start();
    }
    chunks
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out: String = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect();
    out.push('…');
    out
}

pub fn ts(ms: i64, style: char) -> String {
    format!("<t:{}:{style}>", ms.div_euclid(1000))
}

/// Default_Ignorable_Code_Point, which includes Hangul fillers that count as letters.
pub fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF0}'..='\u{FFF8}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

pub fn shows_text(text: &str) -> bool {
    text.chars()
        .any(|c| c.is_alphanumeric() && !is_invisible(c))
}

/// Discord shows backslash escapes in link labels literally, so such characters are dropped.
pub fn link_label(text: &str) -> String {
    text.chars()
        .filter_map(|ch| match ch {
            '[' => Some('('),
            ']' => Some(')'),
            '|' => Some('-'),
            '*' | '_' | '`' | '~' | '\\' => None,
            // A line break would end the link and the `-#` line.
            other if other.is_whitespace() => Some(' '),
            other => Some(other),
        })
        .collect()
}

pub fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(
            ch,
            '*' | '_' | '`' | '~' | '|' | '>' | '#' | '[' | ']' | '(' | ')' | '\\'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

pub fn count_components(components: &[Component]) -> usize {
    components
        .iter()
        .map(|c| {
            1 + match c {
                Component::Container(c) => count_components(&c.components),
                Component::ActionRow(r) => count_components(&r.components),
                Component::Section(s) => count_components(&s.components) + 1,
                _ => 0,
            }
        })
        .sum()
}

pub fn validate_message(components: &[Component]) -> Result<(), String> {
    let count = count_components(components);
    if count > 40 {
        return Err(format!("{count} components (max 40)"));
    }
    let mut total = 0;
    validate_tree(components, &mut total)?;
    if total > 4000 {
        return Err(format!("{total} characters of text (max 4000)"));
    }
    Ok(())
}

fn validate_tree(components: &[Component], total: &mut usize) -> Result<(), String> {
    for component in components {
        match component {
            Component::Container(c) => validate_tree(&c.components, total)?,
            Component::Section(s) => {
                if s.components.is_empty() || s.components.len() > 3 {
                    return Err(format!(
                        "section with {} text blocks",
                        s.components.len()
                    ));
                }
                validate_tree(&s.components, total)?;
                validate_tree(std::slice::from_ref(&s.accessory), total)?;
            }
            Component::ActionRow(row) => {
                if row.components.is_empty() || row.components.len() > 5 {
                    return Err(format!(
                        "action row with {} components",
                        row.components.len()
                    ));
                }
                validate_tree(&row.components, total)?;
            }
            Component::TextDisplay(t) => {
                let len = t.content.chars().count();
                if len > 2000 {
                    return Err(format!("text block of {len} characters"));
                }
                *total += len;
            }
            Component::Button(b) => {
                if b.custom_id
                    .as_ref()
                    .is_some_and(|id| id.len() > 100)
                {
                    return Err("button custom_id over 100".into());
                }
                if b.label
                    .as_ref()
                    .is_some_and(|l| l.chars().count() > 80)
                {
                    return Err("button label over 80".into());
                }
                if b.custom_id.is_none() && b.url.is_none() {
                    return Err("button without custom_id or url".into());
                }
                if b.url
                    .as_ref()
                    .is_some_and(|u| u.len() > 512)
                {
                    return Err("button url over 512".into());
                }
            }
            Component::SelectMenu(m) => {
                if m.custom_id.len() > 100 {
                    return Err("select custom_id over 100".into());
                }
                if m.placeholder
                    .as_ref()
                    .is_some_and(|p| p.chars().count() > 150)
                {
                    return Err("select placeholder over 150".into());
                }
            }
            Component::MediaGallery(g) if g.items.is_empty() || g.items.len() > 10 => {
                return Err(format!("gallery with {} items", g.items.len()));
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_text_prefers_paragraph_breaks_and_respects_limit() {
        let text = format!("{}\n\n{}", "ก".repeat(30), "ข".repeat(30));
        let chunks = split_text(&text, 40);
        assert_eq!(chunks, vec!["ก".repeat(30), "ข".repeat(30)]);
        let long = "a".repeat(95);
        assert!(
            split_text(&long, 40)
                .iter()
                .all(|c| c.chars().count() <= 40)
        );
        assert_eq!(split_text(&long, 40).concat(), long);
    }

    #[test]
    fn card_counts_nested_components() {
        let card = Card::new(color::INFO)
            .title("t")
            .text("body")
            .divider()
            .buttons([button(ButtonStyle::Primary, "a", "A"), link("https://x.y", "X")])
            .build();
        assert_eq!(count_components(&[card]), 7);
    }

    #[test]
    fn small_prefixes_each_line() {
        let card = Card::new(0).small("a\n\nb");
        assert_eq!(card.text_len(), "-# a\n-# b".chars().count());
    }

    #[test]
    fn helpers_format_and_escape() {
        assert_eq!(ts(1_700_000_000_999, 'R'), "<t:1700000000:R>");
        assert_eq!(escape_markdown("a*b_c"), "a\\*b\\_c");
        assert_eq!(truncate("abcdef", 4), "abc…");
    }

    #[test]
    fn link_labels_have_no_escapes_or_brackets() {
        assert_eq!(
            link_label("[Guide] Warframe *beginner* | first_frame `x`"),
            "(Guide) Warframe beginner - firstframe x"
        );
        assert_eq!(link_label("a\nb\r\nc"), "a b  c");
    }
}
