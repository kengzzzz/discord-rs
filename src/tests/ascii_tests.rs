use crate::utils::ascii::{
    ascii_eq_ignore_case, ascii_fold, ascii_lower, ascii_starts_with_icase, ascii_upper,
    cmp_ignore_ascii_case, collect_prefix_icase,
};

#[test]
fn ascii_fold_lowercases() {
    assert_eq!(ascii_fold(b'A'), b'a');
    assert_eq!(ascii_fold(b'Z'), b'z');
    assert_eq!(ascii_fold(b'a'), b'a');
    assert_eq!(ascii_fold(b'!'), b'!');

    assert_eq!(ascii_lower(b'A'), b'a');
    assert_eq!(ascii_lower(b'b'), b'b');
    assert_eq!(ascii_upper(b'a'), b'A');
    assert_eq!(ascii_upper(b'C'), b'C');
}

#[test]
fn ignore_case_comparisons() {
    use std::cmp::Ordering::*;

    assert!(ascii_eq_ignore_case("TeSt", "tEsT"));
    assert!(!ascii_eq_ignore_case("TeSt", "test1"));

    assert_eq!(cmp_ignore_ascii_case("abc", "ABC"), Equal);
    assert_eq!(cmp_ignore_ascii_case("abc", "abd"), Less);
    assert_eq!(cmp_ignore_ascii_case("abe", "abd"), Greater);
}

#[test]
fn prefix_search_helpers() {
    let data = vec![
        "Apple".to_string(),
        "Apricot".to_string(),
        "Banana".to_string(),
        "Berry".to_string(),
        "Carrot".to_string(),
    ];

    assert!(ascii_starts_with_icase("Discord", "dis"));
    assert!(!ascii_starts_with_icase("Discord", "cord"));

    let result = collect_prefix_icase(&data, "ap", |s| s);
    assert_eq!(result, vec!["Apple".to_string(), "Apricot".to_string()]);

    let empty: Vec<String> = collect_prefix_icase(&data, "z", |s| s);
    assert!(empty.is_empty());
}
