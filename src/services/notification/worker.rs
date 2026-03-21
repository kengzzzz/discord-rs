use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Datelike, TimeZone as _, Utc};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use twilight_model::id::{Id, marker::ChannelMarker};

use crate::{configs::notifications::NOTIFICATIONS, context::Context};

pub(crate) fn next_monday_duration_from(now: DateTime<Utc>) -> Duration {
    let w = now.weekday().number_from_monday();
    let days_ahead = 8 - w;
    let next_monday = (now.date_naive() + chrono::Duration::days(days_ahead as i64))
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let target = Utc.from_utc_datetime(&next_monday);
    let duration = (target - now).to_std().unwrap();
    duration + std::time::Duration::from_secs(1)
}

pub(crate) fn notify_loop(
    ctx: &Arc<Context>,
    channel_id: Id<ChannelMarker>,
    role_id: u64,
    message: &str,
    mut calc_delay: impl FnMut() -> Duration + Send + 'static,
    token: CancellationToken,
) -> JoinHandle<()> {
    let msg = message.to_string();
    let ctx = ctx.clone();

    const SEC_48_HOURS: u64 = 48 * 60 * 60;
    const SEC_24_HOURS: u64 = 24 * 60 * 60;
    const SEC_120_HOURS: u64 = 120 * 60 * 60;

    tokio::spawn(async move {
        loop {
            let mut delay = calc_delay();
            while delay > Duration::from_secs(60) {
                let sleep_time = if delay > Duration::from_secs(SEC_48_HOURS) {
                    Duration::from_secs(SEC_24_HOURS)
                } else if delay > Duration::from_secs(600) {
                    Duration::from_secs(600)
                } else {
                    Duration::from_secs(30)
                };

                tokio::select! {
                    _ = token.cancelled() => return,
                    _ = tokio::time::sleep(sleep_time) => {
                        delay = calc_delay();
                    }
                }
            }

            tokio::select! {
                _ = token.cancelled() => return,
                _ = tokio::time::sleep(delay) => {
                    if let Err(e) = ctx.http
                        .create_message(channel_id)
                        .content(&format!("{msg} <@&{role_id}>"))
                        .await
                    {
                        tracing::warn!(
                            channel_id = channel_id.get(),
                            role_id,
                            error = %e,
                            "failed to send notification",
                        );
                    }
                    tokio::select! {
                        _ = token.cancelled() => return,
                        _ = tokio::time::sleep(Duration::from_secs(SEC_120_HOURS)) => {}
                    }
                }
            }
        }
    })
}

pub(crate) fn notify_umbra_loop(
    ctx: &Arc<Context>,
    channel_id: Id<ChannelMarker>,
    role_id: u64,
    mut rx: watch::Receiver<bool>,
    token: CancellationToken,
) -> JoinHandle<()> {
    let ctx = ctx.clone();
    tokio::spawn(async move {
        let mut last = *rx.borrow();
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                changed = rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let now_state = *rx.borrow();
                    if now_state && !last
                        && let Err(e) = ctx.http
                            .create_message(channel_id)
                            .content(&format!("{} <@&{role_id}>", NOTIFICATIONS.umbra_forma))
                            .await
                        {
                            tracing::warn!(
                                channel_id = channel_id.get(),
                                role_id,
                                error = %e,
                                "failed to send umbra forma notification",
                            );
                        }
                    last = now_state;
                }
            }
        }
    })
}
