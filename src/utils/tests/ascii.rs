use super::*;

#[test]
fn test_cmp_ignore_ascii_case() {
    assert_eq!(cmp_ignore_ascii_case("abc", "ABC"), Ordering::Equal);
    assert_eq!(cmp_ignore_ascii_case("abc", "abd"), Ordering::Less);
    assert_eq!(cmp_ignore_ascii_case("abd", "abc"), Ordering::Greater);
}

#[test]
fn test_ascii_starts_with_icase() {
    assert!(ascii_starts_with_icase("HelloWorld", "hello"));
    assert!(ascii_starts_with_icase("HELLO", "hell"));
    assert!(!ascii_starts_with_icase("ello", "hello"));
    assert!(!ascii_starts_with_icase("WorldHello", "hello"));
}

#[test]
fn test_ascii_contains_icase() {
    assert!(ascii_contains_icase("HelloWorld", "world"));
    assert!(ascii_contains_icase("HelloWorld", "WORLD"));
    assert!(!ascii_contains_icase("HelloWorld", "planet"));
    assert!(ascii_contains_icase("anything", ""));
}

#[test]
fn test_collect_prefix_icase() {
    let data: Vec<String> = (0..30).map(|i| format!("item{i:02}")).collect();
    let out = collect_prefix_icase(&data, "", |s| s.as_str());
    assert_eq!(out.len(), 25);
    assert_eq!(out.first().unwrap(), "item00");
    assert_eq!(out.last().unwrap(), "item24");

    let mut words = vec![
        "Apple".to_string(),
        "apricot".to_string(),
        "banana".to_string(),
        "blueberry".to_string(),
        "cherry".to_string(),
    ];
    words.sort_by(|a, b| cmp_ignore_ascii_case(a, b));

    let matches = collect_prefix_icase(&words, "ap", |s| s.as_str());
    assert_eq!(matches, vec!["Apple".to_string(), "apricot".to_string()]);

    let no_matches = collect_prefix_icase(&words, "zzz", |s| s.as_str());
    assert!(no_matches.is_empty());
}
