use crate::utils::ascii::{
    ascii_contains_icase, ascii_lower, ascii_starts_with_icase, cmp_ignore_ascii_case,
    collect_prefix_icase,
};

#[test]
fn ascii_fold_lowercases() {
    assert_eq!(ascii_lower(b'A'), b'a');
    assert_eq!(ascii_lower(b'Z'), b'z');
    assert_eq!(ascii_lower(b'a'), b'a');
    assert_eq!(ascii_lower(b'!'), b'!');
}

#[test]
fn ignore_case_comparisons() {
    use std::cmp::Ordering::*;

    assert_eq!(cmp_ignore_ascii_case("abc", "ABC"), Equal);
    assert_eq!(cmp_ignore_ascii_case("abc", "abd"), Less);
    assert_eq!(cmp_ignore_ascii_case("abe", "abd"), Greater);
}

#[test]
fn prefix_search_helpers() {
    let mut data = vec![
        "APP".to_string(),
        "Apple".to_string(),
        "Apricot".to_string(),
        "apricot2".to_string(),
        "Banana".to_string(),
        "beta".to_string(),
    ];

    assert!(ascii_starts_with_icase("HelloWorld", "heLLo"));
    assert!(ascii_starts_with_icase("abc", ""));
    assert!(!ascii_starts_with_icase("abc", "abcd"));
    assert!(!ascii_starts_with_icase("abc", "abd"));
    assert!(ascii_starts_with_icase("éclair", "é"));
    assert!(!ascii_starts_with_icase("Éclair", "é"));

    assert!(ascii_contains_icase("anything", ""));
    assert!(!ascii_contains_icase("abc", "abcd"));
    assert!(ascii_contains_icase("foobar", "foo"));
    assert!(ascii_contains_icase("foobar", "foobar"));
    assert!(ascii_contains_icase("FoObAr", "foo"));
    assert!(ascii_contains_icase("FoObAr", "BAR"));
    assert!(ascii_contains_icase("FoObAr", "oBa"));
    assert!(ascii_contains_icase("zzzzHELLO", "hello"));
    assert!(!ascii_contains_icase("abcdef", "gh"));
    assert!(!ascii_contains_icase("abcdef", "abd"));
    assert!(ascii_contains_icase("aaaaaa", "AaAa"));
    assert!(ascii_contains_icase("abababa", "BABA"));
    assert!(ascii_contains_icase("éclair", "é"));
    assert!(!ascii_contains_icase("Éclair", "é"));

    data.sort_by(|a, b| cmp_ignore_ascii_case(a, b));

    let result = collect_prefix_icase(&data, "ap", |s| s);
    assert_eq!(
        result,
        vec![
            "APP".to_string(),
            "Apple".to_string(),
            "Apricot".to_string(),
            "apricot2".to_string(),
        ]
    );

    let result_up = collect_prefix_icase(&data, "AP", |s| s);
    assert_eq!(result_up, result);

    let beta = collect_prefix_icase(&data, "b", |s| s);
    assert_eq!(beta, vec!["Banana".to_string(), "beta".to_string()]);

    let empty = collect_prefix_icase(&data, "z", |s| s);
    assert!(empty.is_empty());

    let all_prefix = collect_prefix_icase(&data, "", |s| s);
    assert_eq!(all_prefix, data);
}
