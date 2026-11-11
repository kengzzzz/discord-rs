use std::{
    hash::{DefaultHasher, Hash as _, Hasher as _},
    sync::Arc,
};

use twilight_model::id::{
    Id,
    marker::{GuildMarker, RoleMarker},
};

use super::{cycles, worldstate::Snapshot};
use crate::{
    app::App,
    db::{self, notify},
    discord::ui,
    guild::{ChannelKind, RoleKind},
};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const CETUS_NIGHT_WINDOW_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, PartialEq, Eq)]
pub struct Due {
    pub kind: &'static str,
    pub role: RoleKind,
    pub period: String,
    pub text: String,
}

pub fn due(snapshot: Option<&Snapshot>, bounty_end: Option<i64>, now: i64) -> Vec<Due> {
    let mut out = Vec::new();
    let steel = cycles::steel_path(now);
    let week = cycles::week_index(now);
    if now - steel.activation < DAY_MS {
        out.push(Due {
            kind: "helminth",
            role: RoleKind::Helminth,
            period: format!("w{week}"),
            text: "รีเซ็ตรายสัปดาห์แล้ว บัพ Helminth เปลี่ยนแล้ว".to_owned(),
        });
        out.push(Due {
            kind: "riven_sliver",
            role: RoleKind::RivenSliver,
            period: format!("w{week}"),
            text: "แลก Riven Sliver กับ Palladino ที่ Iron Wake ได้แล้ว".to_owned(),
        });
    }
    if steel.umbra {
        out.push(Due {
            kind: "umbra_forma",
            role: RoleKind::UmbraForma,
            period: format!("w{week}"),
            text: format!(
                "สัปดาห์นี้ Teshin มี **Umbra Forma Blueprint** (หมด {})",
                ui::ts(steel.expiry, 'R')
            ),
        });
    }
    if let Some(baro) = snapshot.and_then(|s| s.baro.as_ref())
        && baro.activation <= now
        && now < baro.expiry
    {
        out.push(Due {
            kind: "baro",
            role: RoleKind::Baro,
            period: baro.activation.to_string(),
            text: format!(
                "**Baro Ki'Teer** มาถึงแล้วที่ {} อยู่ถึง {}",
                baro.relay,
                ui::ts(baro.expiry, 'R')
            ),
        });
    }
    if let Some(bounty) = bounty_end {
        let cetus = cycles::cetus(bounty, now);
        if cetus.state == "night" && now - cetus.activation < CETUS_NIGHT_WINDOW_MS {
            out.push(Due {
                kind: "cetus_night",
                role: RoleKind::CetusNight,
                period: cetus.activation.to_string(),
                text: format!(
                    "**Cetus** กลางคืนแล้ว ล่า Eidolon ได้ถึง {}",
                    ui::ts(cetus.expiry, 't')
                ),
            });
        }
    }
    out
}

pub async fn tick(app: &Arc<App>) {
    let snapshot = app.warframe.snapshot();
    let now = jiff::Timestamp::now().as_millisecond();
    let bounty = super::status::bounty_end(app, snapshot.as_deref(), now);
    let due = due(snapshot.as_deref(), bounty, now);
    if due.is_empty() {
        return;
    }
    for (guild, channel) in app
        .guilds
        .with_channel(ChannelKind::Notification)
    {
        if app.shutdown.is_cancelled() {
            break;
        }
        let config = app.guild(guild);
        for item in &due {
            let Some(role) = config.role_id(item.role) else {
                continue;
            };
            let (kind, period) = (item.kind, item.period.clone());
            let claimed = app
                .db
                .call(move |c| notify::mark_pending(c, guild, kind, &period, db::now()))
                .await;
            match claimed {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    tracing::warn!(%error, "notification claim failed");
                    continue;
                }
            }
            let body = payload(item, role, guild);
            let period = item.period.clone();
            match app
                .http
                .create_message(channel)
                .payload_json(&body)
                .await
            {
                Ok(response) => {
                    let message = crate::discord::read(response)
                        .await
                        .ok()
                        .map(|m| m.id);
                    let _ = app
                        .db
                        .call(move |c| {
                            notify::mark_sent(c, guild, kind, &period, message, db::now())
                        })
                        .await;
                    metrics::counter!("notifications_total", "kind" => kind).increment(1);
                }
                // A 4xx won't fix itself; only timeouts and 429s are retried.
                Err(error)
                    if crate::discord::errors::status(&error)
                        .is_some_and(|s| (400..500).contains(&s) && s != 429) =>
                {
                    tracing::warn!(guild = guild.get(), kind, %error, "notification refused by Discord; skipping this period");
                    let _ = app
                        .db
                        .call(move |c| notify::mark_sent(c, guild, kind, &period, None, db::now()))
                        .await;
                }
                Err(error) => {
                    tracing::warn!(guild = guild.get(), kind, %error, "notification send failed; will retry");
                    let _ = app
                        .db
                        .call(move |c| notify::forget(c, guild, kind, &period))
                        .await;
                }
            }
        }
    }
}

/// A timed-out send may have posted. The nonce comes from the content, so with `enforce_nonce`
/// Discord returns the first message instead of posting the retry.
fn payload(item: &Due, role: Id<RoleMarker>, guild: Id<GuildMarker>) -> Vec<u8> {
    let mut hasher = DefaultHasher::new();
    (guild, item.kind, &item.period).hash(&mut hasher);
    serde_json::to_vec(&serde_json::json!({
        "content": format!("{} <@&{role}>", item.text),
        "allowed_mentions": { "parse": [], "roles": [role.to_string()] },
        "nonce": format!("n{:016x}", hasher.finish()),
        "enforce_nonce": true,
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warframe::worldstate::Baro;

    const MONDAY_UMBRA: i64 = 1_794_182_400_000;

    fn kinds(due: &[Due]) -> Vec<&'static str> {
        due.iter().map(|d| d.kind).collect()
    }

    #[test]
    fn weekly_pings_only_in_first_day_and_umbra_all_week() {
        assert_eq!(
            kinds(&due(None, None, MONDAY_UMBRA + 1_000)),
            ["helminth", "riven_sliver", "umbra_forma"]
        );
        assert_eq!(
            kinds(&due(None, None, MONDAY_UMBRA + 3 * DAY_MS)),
            ["umbra_forma"]
        );
        assert!(due(None, None, MONDAY_UMBRA - 2 * DAY_MS).is_empty());
        let a = due(None, None, MONDAY_UMBRA + 1_000);
        let b = due(None, None, MONDAY_UMBRA + 5 * 60 * 60 * 1000);
        assert_eq!(a[0].period, b[0].period);
    }

    #[test]
    fn baro_and_cetus_night_are_keyed_by_activation() {
        let now = MONDAY_UMBRA + 2 * DAY_MS;
        let snapshot = Snapshot {
            baro: Some(Baro {
                activation: now - 1000,
                expiry: now + DAY_MS,
                relay: "Kronia Relay (Saturn)".into(),
                item_count: 3,
            }),
            ..Snapshot::default()
        };
        let bounty = now + 3_000_000 - 60_000;
        let due = due(Some(&snapshot), Some(bounty), now);
        assert!(kinds(&due).contains(&"baro"));
        assert!(kinds(&due).contains(&"cetus_night"));
        assert_eq!(
            due.iter()
                .find(|d| d.kind == "baro")
                .unwrap()
                .period,
            (now - 1000).to_string()
        );
    }

    #[test]
    fn payload_nonce_is_stable_per_guild_and_period() {
        let items = due(None, None, MONDAY_UMBRA + 1_000);
        let parse = |body: Vec<u8>| serde_json::from_slice::<serde_json::Value>(&body).unwrap();
        let a = parse(payload(&items[0], Id::new(7), Id::new(1)));
        let again = parse(payload(&items[0], Id::new(7), Id::new(1)));
        let other_guild = parse(payload(&items[0], Id::new(7), Id::new(2)));
        let other_kind = parse(payload(&items[1], Id::new(7), Id::new(1)));
        assert_eq!(a["nonce"], again["nonce"]);
        assert_ne!(a["nonce"], other_guild["nonce"]);
        assert_ne!(a["nonce"], other_kind["nonce"]);
        assert!(a["nonce"].as_str().unwrap().len() <= 25);
        assert_eq!(a["enforce_nonce"], true);
        assert_eq!(a["allowed_mentions"]["roles"][0], "7");
        assert!(
            a["content"]
                .as_str()
                .unwrap()
                .ends_with("<@&7>")
        );
    }
}
