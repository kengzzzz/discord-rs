use super::client::MongoDB;
use mongodb::Client;

impl MongoDB {
    pub async fn empty() -> Self {
        let client = Client::with_uri_str("mongodb://localhost:27017")
            .await
            .unwrap();
        let db = client.database("test");
        Self {
            client,
            channels: db.collection("channels"),
            roles: db.collection("roles"),
            quarantines: db.collection("quarantines"),
            messages: db.collection("messages"),
            ai_prompts: db.collection("ai_prompts"),
        }
    }
}
