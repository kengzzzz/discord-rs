use std::sync::Arc;

use discord_bot::context::{Context, ContextBuilder, mock_http::MockClient};

pub async fn build_context() -> Arc<Context> {
    let ctx = ContextBuilder::new()
        .http(MockClient::new())
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
}
