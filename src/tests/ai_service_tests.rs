use google_ai_rs::{
    genai::Response,
    proto::part::Data,
    proto::{Candidate, Content, Part},
};
use twilight_model::id::{Id, marker::UserMarker};

use crate::services::ai::{self, AiService};

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
    ai::set_generate_override(|_| mock_response("ok"));

    AiService::clear_history(user).await;
    AiService::set_prompt(user, "hi".to_string()).await;

    let text = AiService::handle_interaction(user, "Tester", "hello", None)
        .await
        .unwrap();
    assert_eq!(text, "ok");

    let hist = ai::load_history_test(user).await;
    assert_eq!(hist.len(), 2);
    assert_eq!(ai::entry_role(&hist[0]), "user");

    let prompt = ai::get_prompt_test(user).await;
    assert_eq!(prompt, Some("hi".to_string()));
}

#[tokio::test]
async fn test_summary_rotation() {
    let user = Id::<UserMarker>::new(2);
    ai::set_generate_override(|_| mock_response("ok"));
    ai::set_summarize_override(|_| "SUM".to_string());

    let history: Vec<_> = (0..21)
        .map(|i| ai::new_entry("user", &format!("m{i}")))
        .collect();
    ai::set_history_test(user, history).await;

    let _ = AiService::handle_interaction(user, "Tester", "msg", None).await;
    let hist = ai::load_history_test(user).await;
    assert_eq!(hist.len(), 9); // summary + KEEP_RECENT + 2
    assert_eq!(ai::entry_role(&hist[0]), "system");
    assert!(ai::entry_text(&hist[0]).contains("SUM"));
}
