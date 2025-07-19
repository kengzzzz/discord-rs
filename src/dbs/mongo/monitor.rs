use mongodb::bson::doc;
use tokio::time::{self, Duration};

use crate::{
    dbs::mongo::MongoDB,
    services::{health::HealthService, shutdown},
};

pub fn spawn_monitor(db: MongoDB) {
    tokio::spawn(async move {
        let token = shutdown::get_token();
        let mut interval = time::interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = interval.tick() => {},
            }

            let ok = db
                .client()
                .database("admin")
                .run_command(doc! { "ping": 1 })
                .await
                .is_ok();

            HealthService::set_mongo(ok);
        }
    });
}
