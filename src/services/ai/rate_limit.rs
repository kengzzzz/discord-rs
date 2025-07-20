use std::sync::Arc;

use chrono::Utc;
use twilight_model::id::{Id, marker::UserMarker};

use crate::{configs::CACHE_PREFIX, context::Context};

const RATE_LIMIT_SECS: i64 = 3;

pub(crate) async fn check_rate_limit(ctx: &Arc<Context>, user: Id<UserMarker>) -> Option<u64> {
    let key = format!("{CACHE_PREFIX}:ai:rate:{}", user.get());
    let now = Utc::now().timestamp();
    if let Some(last) = ctx.redis_get::<i64>(&key).await {
        let diff = now - last;
        if diff < RATE_LIMIT_SECS {
            return Some((RATE_LIMIT_SECS - diff) as u64);
        }
    }
    ctx.redis_set_ex(&key, &now, RATE_LIMIT_SECS as usize)
        .await;
    None
}

#[cfg(all(test, not(feature = "test-utils")))]
#[path = "tests/rate_limit.rs"]
mod tests;
