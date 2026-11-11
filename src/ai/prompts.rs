pub const TRANSLATE: &str = "\
You are a professional translator for a Thai gaming community on Discord.
Translate every piece of text in the provided image(s) and/or message into natural, fluent Thai.

Rules:
- Output only the translation. No preamble, no explanations unless the user asks. Do not add emoji.
- Keep the original structure: line breaks, lists, speaker labels. For images, follow reading order and group text by panel, bubble or UI section.
- The output must be Thai. Translate every word that is not a name. Keep only names (people, VTubers, channels, hashtags, items, game terms such as Warframe, Incarnon, Steel Path) in their original form or their usual English/romaji spelling (常闇トワ → Tokoyami Towa, ホロライブ → hololive). Do not translate usernames.
- Never return the source text unchanged, even when it is mostly names: a stream or video title still has words to translate (最終回 → ตอนสุดท้าย).
- Write Warframe names in their official English spelling even when the source uses katakana or Thai script (アルコンハント → Archon Hunt, スチールパス → Steel Path).
- Japanese: translate the meaning naturally, not word-by-word; keep honorific nuance when it matters.
- If some text is unreadable or cut off, write [อ่านไม่ออก] instead of guessing. Never invent text that is not there.
- If the source is already Thai, translate it into English instead.
- Follow any extra instruction the user gives (for example: include romaji / คำอ่าน, or translate into another language).";

/// CJK text with no Thai letters was copied, not translated (some models return it unchanged).
pub fn looks_translated(text: &str) -> bool {
    let mut thai = false;
    let mut cjk = false;
    for ch in text.chars() {
        match ch {
            '\u{0E00}'..='\u{0E7F}' => thai = true,
            '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7AF}' => {
                cjk = true
            }
            _ => {}
        }
    }
    thai || !cjk
}

pub fn chat(
    bot_name: &str,
    user_name: &str,
    today: &str,
    extra: &str,
    persona: Option<&str>,
) -> String {
    let mut prompt = format!(
        "\
You are {bot_name}, the assistant of a Thai Warframe community Discord server.
You are talking with {user_name}. Today is {today}.

Style:
- Reply in the user's language (usually Thai). Be friendly, concise and practical.
- Use Discord markdown: **bold**, bullet lists, `code`, short headings with ###. Never use tables.
- Answer the question directly in the first sentence, then add only the points that matter.
- Keep answers under about 150 words unless the user asks for detail.
- Do not use emoji or emoticons.

Community context:
- Members talk mostly about Warframe. Thai slang: บิว/บิ้ว = build, ฟาร์ม = farm, เฟรม = Warframe, ฟอร์มา = Forma, \
อินคาร์นอน/Incarnon = Incarnon Genesis weapon upgrade, SP = Steel Path, ริเวน = Riven mod, เพลต/แพลต = platinum.
- Names like Hate, Soma, Braton, Kuva Bramma are Warframe weapons; Excalibur, Saryn, Wisp are Warframes.
- For builds, give a sensible mod setup and explain the idea briefly. This bot also has Discord slash commands \
(typed here in Discord, not in the game): /warframe build shows community builds and /warframe market shows live prices.
- If you are not sure about current game numbers or patch changes, say so instead of guessing.
- You can see images users attach. If asked to translate, translate faithfully."
    );
    if !extra.trim().is_empty() {
        prompt.push_str("\n\nServer instructions:\n");
        prompt.push_str(extra.trim());
    }
    if let Some(persona) = persona.filter(|p| !p.trim().is_empty()) {
        prompt.push_str("\n\nThe user asked you to follow this personal style (do not break the rules above):\n");
        prompt.push_str(persona.trim());
    }
    prompt
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranslateRequest {
    pub source: Option<String>,
    pub instruction: String,
}

const EXACT: [&str; 9] =
    ["แปล", "แปลไทย", "แปลหน่อย", "แปลให้หน่อย", "แปลเป็นไทย", "แปลด้วย", "translate", "tl", "tr"];
const TRAILING: [&str; 8] =
    ["แปลว่าอะไร", "แปลให้หน่อย", "แปลหน่อย", "แปลไทย", "แปลว่า", "แปลด้วย", "แปล", "translate"];
const LEADING: [&str; 2] = ["แปล", "translate"];

fn is_thai(ch: char) -> bool {
    ('\u{0E00}'..='\u{0E7F}').contains(&ch)
}

fn strip_prefix_ci<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let head = text.get(..keyword.len())?;
    head.eq_ignore_ascii_case(keyword)
        .then(|| &text[keyword.len()..])
}

fn strip_suffix_ci<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let cut = text.len().checked_sub(keyword.len())?;
    let tail = text.get(cut..)?;
    tail.eq_ignore_ascii_case(keyword)
        .then(|| &text[..cut])
}

pub fn translate_request(text: &str, has_context: bool) -> Option<TranslateRequest> {
    let trimmed = text.trim();
    if EXACT
        .iter()
        .any(|k| trimmed.eq_ignore_ascii_case(k))
    {
        return Some(TranslateRequest { source: None, instruction: String::new() });
    }
    for keyword in TRAILING {
        let Some(prefix) = strip_suffix_ci(trimmed, keyword) else { continue };
        let separated = prefix
            .chars()
            .next_back()
            .is_some_and(|c| c.is_whitespace() || !is_thai(c));
        let source = prefix.trim();
        if separated && !source.is_empty() && source.chars().count() <= 1500 {
            return Some(TranslateRequest {
                source: Some(source.to_owned()),
                instruction: String::new(),
            });
        }
    }
    for keyword in LEADING {
        let Some(raw) = strip_prefix_ci(trimmed, keyword) else { continue };
        if !translate_continues(raw) {
            continue;
        }
        if has_context {
            return (trimmed.chars().count() <= 80)
                .then(|| TranslateRequest { source: None, instruction: raw.trim().to_owned() });
        }
        let source = raw
            .trim_start_matches(|c: char| c == ':' || c.is_whitespace())
            .trim_end();
        let separated = raw.starts_with(|c: char| c == ':' || c.is_whitespace());
        if !source.is_empty() && separated {
            return Some(TranslateRequest {
                source: Some(source.to_owned()),
                instruction: String::new(),
            });
        }
    }
    None
}

/// Thai has no spaces, so "แปลกๆ" (odd) and "แปลง" (convert) start with "แปล". Only known
/// continuations of "translate" count.
fn translate_continues(rest: &str) -> bool {
    const WORDS: [&str; 27] = [
        "เป็น",
        "ให้",
        "ไทย",
        "หน่อย",
        "ด้วย",
        "นี้",
        "นี่",
        "คำ",
        "ข้อความ",
        "รูป",
        "ภาพ",
        "บท",
        "การ์ตูน",
        "ว่า",
        "ที",
        "อัน",
        "ตรง",
        "ทั้ง",
        "จาก",
        "ภาษา",
        "อังกฤษ",
        "งาน",
        "ครับ",
        "ค่ะ",
        "คะ",
        "จ้า",
        "เลย",
    ];
    rest.is_empty()
        || rest.starts_with(|c: char| c.is_whitespace() || !is_thai(c))
        || WORDS
            .iter()
            .any(|w| rest.starts_with(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odd_and_convert_are_not_translate_requests() {
        for text in ["แปลกๆ นะ ทำไม build นี้ dps ต่ำ", "แปลง riven ยังไง", "แปลกใจ"]
        {
            assert_eq!(translate_request(text, true), None, "{text}");
        }
        for text in
            ["แปล", "แปลนี้หน่อย", "แปลบทความนี้", "แปลการ์ตูนนี้", "แปลให้หน่อย", "แปลงานนี้", "แปล: hello"]
        {
            assert!(translate_request(text, true).is_some(), "{text}");
        }
    }

    fn source(text: &str, context: bool) -> Option<String> {
        translate_request(text, context).and_then(|r| r.source)
    }

    #[test]
    fn detects_real_translate_requests_from_the_server() {
        assert_eq!(
            translate_request("แปล", true),
            Some(TranslateRequest { source: None, instruction: String::new() })
        );
        assert_eq!(
            source("つづり郵便局  แปล", false).as_deref(),
            Some("つづり郵便局")
        );
        assert_eq!(
            source("図鑑コンプ目指す釣り配信  แปล", false).as_deref(),
            Some("図鑑コンプ目指す釣り配信")
        );
        assert_eq!(
            source("ソロライブ情報解禁‼️初の3D新衣装お披露目ライブ＆告知アリ‼️こよりわかり手選手権 แปล", false).as_deref(),
            Some("ソロライブ情報解禁‼️初の3D新衣装お披露目ライブ＆告知アリ‼️こよりわかり手選手権")
        );
        assert_eq!(
            source("clout แปลว่า", false).as_deref(),
            Some("clout")
        );
        assert_eq!(source("配信แปล", false).as_deref(), Some("配信"));
        assert_eq!(
            translate_request("แปล text แบบไวยากรณ์ไทย", true)
                .map(|r| r.instruction)
                .as_deref(),
            Some("text แบบไวยากรณ์ไทย")
        );
        assert_eq!(
            translate_request("แปลเป็นคำอ่านให้ด้วย", true)
                .map(|r| r.instruction)
                .as_deref(),
            Some("เป็นคำอ่านให้ด้วย")
        );
        assert_eq!(
            source("แปล ありがとう", false).as_deref(),
            Some("ありがとう")
        );
        assert_eq!(
            source("translate: thank you", false).as_deref(),
            Some("thank you")
        );
    }

    #[test]
    fn ignores_normal_questions_and_thai_words_ending_in_the_keyword() {
        assert_eq!(
            translate_request("ขอบิว hate incarnon หน่อย", false),
            None
        );
        assert_eq!(translate_request("ไม่เข้าใจคำแปล", false), None);
        assert_eq!(
            translate_request("คิดว่าคืออาหารอะไร ในญี่ปุ่น", true),
            None
        );
        assert_eq!(translate_request("แปลกมาก", false), None);
    }

    #[test]
    fn chat_prompt_layers_server_and_persona() {
        let prompt = chat(
            "Uncle Bot",
            "keng",
            "2026-11-08",
            "be nice",
            Some("speak like Arona"),
        );
        assert!(
            prompt.contains("Uncle Bot") && prompt.contains("be nice") && prompt.contains("Arona")
        );
        assert!(!chat("b", "u", "d", "", None).contains("personal style"));
    }

    #[test]
    fn copied_cjk_text_is_not_a_translation() {
        assert!(!looks_translated(
            "【#ホ口鯖ARK】最終回 | 常闇ファミリアでBOSS挑戦するぞ！！【常闇トワ/ホロライブ】"
        ));
        assert!(!looks_translated("오늘 같이 레이드 가실 분"));
        assert!(!looks_translated("今晚一起打副本吗"));
        assert!(looks_translated(
            "【#HoloServerARK】ตอนสุดท้าย | จะท้าดวลบอส 【Tokoyami Towa/hololive】"
        ));
        assert!(looks_translated("ตอนสุดท้ายของ 常闇トワ"));
        assert!(looks_translated(
            "The servers will be down tomorrow."
        ));
    }
}
