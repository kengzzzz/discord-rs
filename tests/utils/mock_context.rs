use std::sync::Arc;

use super::mock_http::MockClient;
use super::mock_reqwest::MockReqwest;
use discord_bot::context::{Context, ContextBuilder, mock_http};

pub async fn build_context() -> Arc<Context> {
    let ctx = ContextBuilder::new()
        .http(mock_http::MockClient::new())
        .reqwest(MockReqwest::new())
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
}
