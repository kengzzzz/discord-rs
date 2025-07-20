pub use discord_bot::context::mock_http::{
    InteractionRecord, MessageOp, MessageRecord, MockClient,
};

pub fn last_interaction(client: &MockClient) -> Option<InteractionRecord> {
    client.interactions.lock().unwrap().last().cloned()
}

pub fn last_message(client: &MockClient) -> Option<MessageRecord> {
    client.messages.lock().unwrap().last().cloned()
}
