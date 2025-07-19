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
            out.push(first.to_ascii_uppercase() as char);
            for b in bytes {
                out.push(b.to_ascii_lowercase() as char);
            }
        }
    }

    out.push_str("** ends");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_time_valid() {
        let s = "2024-01-02T03:04:05Z";
        let ts = chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp();
        assert_eq!(format_time(s), format!("<t:{ts}:R>"));
    }

    #[test]
    fn test_format_time_invalid() {
        assert_eq!(format_time("invalid"), "");
    }

    #[test]
    fn test_title_case_basic() {
        assert_eq!(title_case("hello world"), "**Hello World** ends");
    }

    #[test]
    fn test_title_case_mixed() {
        assert_eq!(title_case("hELLo   WoRLD"), "**Hello World** ends");
    }
}
