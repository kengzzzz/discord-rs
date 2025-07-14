pub fn format_time(s: &str) -> String {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        format!("<t:{}:R>", t.timestamp())
    } else {
        String::new()
    }
}

pub fn title_case(s: &str) -> String {
    let mut out = String::new();
    for (i, part) in s.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = part.chars();
        if let Some(f) = chars.next() {
            out.push_str(&format!(
                "**{}{}",
                f.to_uppercase(),
                chars.as_str().to_lowercase()
            ));
        }
    }
    out.push_str("** ends");
    out
}
