use google_ai_rs::{
    genai::Response,
    proto::part::Data,
    proto::{Candidate, Content, Part},
};
use twilight_model::id::{Id, marker::UserMarker};

use crate::services::ai::tests::{set_generate_override, set_summarize_override};
use crate::services::ai::{AiService, history, models::ChatEntry};

fn mock_response(text: &str) -> Response {
    Response {
        candidates: vec![Candidate {
            index: Some(0),
            content: Some(Content {
                parts: vec![Part {
                    data: Some(Data::Text(text.to_string())),
                }],
                role: String::new(),
            }),
            finish_reason: 0,
            safety_ratings: Vec::new(),
            citation_metadata: None,
            token_count: 0,
            grounding_attributions: Vec::new(),
            grounding_metadata: None,
            avg_logprobs: 0.0,
            logprobs_result: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
        model_version: String::new(),
    }
}

#[tokio::test]
async fn test_prompt_and_history() {
    let user = Id::<UserMarker>::new(1);
    let ctx = std::sync::Arc::new(crate::context::Context::test().await);
    set_generate_override(|_| mock_response("ok"));

    AiService::clear_history(user).await;
    AiService::set_prompt(ctx.clone(), user, "hi".to_string()).await;

    let text = AiService::handle_interaction(
        ctx.clone(),
        user,
        "Tester",
        "hello",
        Vec::new(),
        None,
        Vec::new(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(text, "ok");

    let hist = history::load_history(user).await;
    assert_eq!(hist.len(), 2);
    assert_eq!(hist[0].role, "user".to_string());

    let prompt = history::get_prompt(ctx.clone(), user).await;
    assert_eq!(prompt, Some("hi".to_string()));
}

#[tokio::test]
async fn test_reply_fields() {
    let user = Id::<UserMarker>::new(10);
    let ctx = std::sync::Arc::new(crate::context::Context::test().await);
    set_generate_override(|_| mock_response("ok"));

    AiService::clear_history(user).await;

    let _ = AiService::handle_interaction(
        ctx.clone(),
        user,
        "Tester",
        "hi",
        Vec::new(),
        Some("hello"),
        Vec::new(),
        Some("Tester2"),
    )
    .await;

    let hist = history::load_history(user).await;
    assert_eq!(hist.len(), 2);
    assert_eq!(hist[0].ref_text, Some("hello".to_string()));
    assert!(hist[0].ref_attachments.is_none());
    assert_eq!(hist[0].ref_author, Some("Tester2".to_string()));
}

#[tokio::test]
async fn test_summary_rotation() {
    let user = Id::<UserMarker>::new(2);
    let ctx = std::sync::Arc::new(crate::context::Context::test().await);
    set_generate_override(|_| mock_response("ok"));
    set_summarize_override(|_| "SUM".to_string());

    let history: Vec<_> = (0..21)
        .map(|i| {
            ChatEntry::new(
                "user".to_string(),
                format!("m{i}"),
                Vec::new(),
                None,
                None,
                None,
            )
        })
        .collect();
    history::store_history(user, &history).await;

    let _ = AiService::handle_interaction(
        ctx.clone(),
        user,
        "Tester",
        "msg",
        Vec::new(),
        None,
        Vec::new(),
        None,
    )
    .await;
    let hist = history::load_history(user).await;
    assert_eq!(hist.len(), 9); // summary + KEEP_RECENT + 2
    assert_eq!(hist[0].role, "user".to_string());
    assert!(hist[0].text.contains("SUM"));
}

#[tokio::test]
async fn test_purge_prompt_cache() {
    let user = Id::<UserMarker>::new(3);
    let ctx = std::sync::Arc::new(crate::context::Context::test().await);
    AiService::set_prompt(ctx.clone(), user, "hello".to_string()).await;
    let prompt = history::get_prompt(ctx.clone(), user).await;
    assert_eq!(prompt, Some("hello".to_string()));

    AiService::purge_prompt_cache(user.get()).await;
    let prompt = history::get_prompt(ctx.clone(), user).await;
    assert!(prompt.is_none());
}
