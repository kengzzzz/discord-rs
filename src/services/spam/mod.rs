use mongodb::bson::doc;
use std::sync::Arc;

use crate::{
    context::Context,
    dbs::redis::{redis_get, redis_set},
};

pub struct SpamService;

pub mod embed;
pub mod log;
pub mod quarantine;

impl SpamService {
    pub async fn is_quarantined(ctx: &Arc<Context>, guild_id: u64, user_id: u64) -> bool {
        let key = format!("spam:quarantine:{guild_id}:{user_id}");
        if redis_get::<String>(&ctx.redis, &key).await.is_some() {
            return true;
        }

        let res = ctx
            .mongo
            .quarantines
            .find_one(doc! {"guild_id": guild_id as i64, "user_id": user_id as i64})
            .await
            .ok()
            .flatten();

        redis_set(&ctx.redis, &key, &res).await;

        res.is_some()
    }
}
