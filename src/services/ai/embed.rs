use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::EmbedBuilder;

use super::AiService;

const COLOR: u32 = 0x5865F2;

impl AiService {
    pub fn ai_embeds(text: &str) -> anyhow::Result<Vec<Embed>> {
        const LIMIT: usize = 1024;
        let mut embeds = Vec::new();
        if text.is_empty() {
            return Ok(embeds);
        }

        let mut remaining = text.trim();
        while !remaining.is_empty() {
            let mut end = remaining
                .char_indices()
                .take(LIMIT)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or_else(|| remaining.len());

            if end < remaining.len() {
                if let Some(pos) = remaining[..end].rfind(|c: char| c.is_whitespace()) {
                    end = pos + 1;
                }
            }

            let chunk = &remaining[..end];
            let embed = EmbedBuilder::new()
                .color(COLOR)
                .description(chunk)
                .validate()?
                .build();
            embeds.push(embed);
            remaining = remaining[end..].trim_start();
        }
        Ok::<_, anyhow::Error>(embeds)
    }

    pub fn rate_limit_embed(wait: u64) -> anyhow::Result<Embed> {
        let embed = EmbedBuilder::new()
            .color(COLOR)
            .title("⏳ คุณส่งข้อความเร็วเกินไป")
            .description(format!("กรุณารอ {wait} วินาทีแล้วลองอีกครั้ง"))
            .validate()?
            .build();
        Ok(embed)
    }
}

#[cfg(test)]
#[path = "tests/embed.rs"]
mod tests;
