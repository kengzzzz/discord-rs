use super::*;
use std::time::Duration;
use twilight_model::id::{Id, marker::ChannelMarker};

pub(crate) fn notify_loop_mock(
    http: std::sync::Arc<crate::tests::mock_http::MockHttp>,
    channel_id: Id<ChannelMarker>,
    role_id: u64,
    message: &str,
    mut calc_delay: impl FnMut() -> Duration + Send + 'static,
    token: CancellationToken,
) -> JoinHandle<()> {
    let msg = message.to_string();
    tokio::spawn(async move {
        loop {
            let delay = calc_delay();
            tokio::select! {
                _ = token.cancelled() => break,
                _ = tokio::time::sleep(delay) => {
                    let _ = http
                        .create_message(channel_id)
                        .content(&format!("{msg} <@&{role_id}>"))
                        .await;
                }
            }
        }
    })
}
