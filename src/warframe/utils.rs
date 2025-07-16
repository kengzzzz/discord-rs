use crate::utils::ascii::{ascii_lower, ascii_upper};

pub fn format_time(s: &str) -> String {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        format!("<t:{}:R>", t.timestamp())
    } else {
        String::new()
    }
}

pub fn title_case(s: &str) -> String {
    let wc = s.split_whitespace().count();
    let mut out = String::with_capacity(s.len() + wc + 4);
    out.push_str("**");

    let mut first_word = true;
    for part in s.split_whitespace() {
        if !first_word {
            out.push(' ');
        } else {
            first_word = false;
        }

        let mut bytes = part.bytes();
        if let Some(first) = bytes.next() {
            out.push(ascii_upper(first) as char);
            for b in bytes {
                out.push(ascii_lower(b) as char);
            }
        }
    }

    out.push_str("** ends");
    out
}
