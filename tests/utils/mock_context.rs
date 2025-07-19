use std::sync::Arc;

use discord_bot::context::{Context, ContextBuilder};

pub async fn build_context() -> Arc<Context> {
    let ctx = ContextBuilder::new()
        .watchers(false)
        .build()
        .await
        .expect("failed to build Context");
    Arc::new(ctx)
}
