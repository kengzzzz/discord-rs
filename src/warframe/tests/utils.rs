use super::*;

#[test]
fn test_format_time_valid() {
    let s = "2024-01-02T03:04:05Z";
    let ts = chrono::DateTime::parse_from_rfc3339(s)
        .unwrap()
        .timestamp();
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
    assert_eq!(
        title_case("hELLo   WoRLD"),
        "**Hello World** ends"
    );
}
