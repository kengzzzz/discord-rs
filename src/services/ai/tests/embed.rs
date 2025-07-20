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
