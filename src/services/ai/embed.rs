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
mod tests {
    use super::*;

    #[test]
    fn test_ai_embeds_empty() {
        let embeds = AiService::ai_embeds("").unwrap();
        assert!(embeds.is_empty());
    }

    #[test]
    fn test_ai_embeds_split_at_whitespace() {
        let text = format!("{} {}", "a".repeat(1020), "bcdefgh");
        let embeds = AiService::ai_embeds(&text).unwrap();
        assert_eq!(embeds.len(), 2);
        assert_eq!(embeds[0].description.as_deref(), Some(&text[..1021]));
        assert_eq!(embeds[1].description.as_deref(), Some("bcdefgh"));
    }

    #[test]
    fn test_rate_limit_embed() {
        let embed = AiService::rate_limit_embed(1).unwrap();
        assert_eq!(embed.title.as_deref(), Some("⏳ คุณส่งข้อความเร็วเกินไป"));
        assert_eq!(
            embed.description.as_deref(),
            Some("กรุณารอ 1 วินาทีแล้วลองอีกครั้ง")
        );
        assert_eq!(embed.color, Some(COLOR));
    }
}
