use super::*;
use chrono::Duration;

fn build_entry(created_at: chrono::DateTime<Utc>) -> ChatEntry {
    ChatEntry {
        role: "user".to_string(),
        text: "hello".to_string(),
        attachments: vec!["https://example.com/file.png".to_string()],
        ref_text: Some("reply".to_string()),
        ref_attachments: Some(vec!["https://example.com/ref.png".to_string()]),
        ref_author: Some("Bob".to_string()),
        created_at,
    }
}

#[tokio::test]
async fn test_parse_history_non_expired() {
    let entry = build_entry(Utc::now() - Duration::hours(1));
    let result = parse_history([&entry], "Alice").await;
    assert_eq!(result.len(), 1);
    let content = &result[0];

    let expected_parts = vec![
        Part::text("hello"),
        Part::text("Attachment from Alice:"),
        Part::file_data("", "https://example.com/file.png"),
        Part::text("In reply to Bob:"),
        Part::text("reply"),
        Part::text("Attachment from Bob:"),
        Part::file_data("", "https://example.com/ref.png"),
    ];

    assert_eq!(content.role, "user");
    assert_eq!(content.parts, expected_parts);
}

#[tokio::test]
async fn test_parse_history_expired() {
    let entry = build_entry(Utc::now() - Duration::hours(72));
    let result = parse_history([&entry], "Alice").await;
    assert_eq!(result.len(), 1);
    let content = &result[0];

    let expected_parts = vec![
        Part::text("hello"),
        Part::text("Attachment from Alice is expired and no longer accessible."),
        Part::text("In reply to Bob:"),
        Part::text("reply"),
        Part::text("Attachment from Bob is expired and no longer accessible."),
    ];

    assert_eq!(content.role, "user");
    assert_eq!(content.parts, expected_parts);
}
