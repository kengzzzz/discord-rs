use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    sync::Arc,
};

use twilight_model::{
    application::interaction::Interaction,
    channel::message::{Component, component::ButtonStyle},
    id::{
        Id,
        marker::{GuildMarker, MessageMarker},
    },
};

use super::{
    circuit,
    cycles::{self, Cycle, CycleKind},
    worldstate::{self, Snapshot},
};
use crate::{
    app::App,
    db::{
        self,
        messages::{BotMessage, BotMessageKind},
    },
    discord::{
        errors, reply,
        ui::{self, Card, color},
    },
    guild::ChannelKind,
};

pub const PREFIX: &str = "status:";
const STALE_AFTER_MS: i64 = 5 * 60 * 1000;

fn cycle_line(cycle: &Cycle) -> String {
    let (name, state) = match (cycle.kind, cycle.state) {
        (CycleKind::Earth, "day") => ("Earth", "กลางวัน"),
        (CycleKind::Earth, _) => ("Earth", "กลางคืน"),
        (CycleKind::Cetus, "day") => ("Cetus", "กลางวัน"),
        (CycleKind::Cetus, _) => ("Cetus", "กลางคืน"),
        (CycleKind::Vallis, "warm") => ("Orb Vallis", "อุ่น"),
        (CycleKind::Vallis, _) => ("Orb Vallis", "หนาว"),
        (CycleKind::Cambion, "fass") => ("Cambion Drift", "Fass"),
        (CycleKind::Cambion, _) => ("Cambion Drift", "Vome"),
        (CycleKind::Zariman, "corpus") => ("Zariman", "Corpus"),
        (CycleKind::Zariman, _) => ("Zariman", "Grineer"),
        (CycleKind::Duviri, "joy") => ("Duviri", "Joy"),
        (CycleKind::Duviri, "anger") => ("Duviri", "Anger"),
        (CycleKind::Duviri, "envy") => ("Duviri", "Envy"),
        (CycleKind::Duviri, "sorrow") => ("Duviri", "Sorrow"),
        (CycleKind::Duviri, _) => ("Duviri", "Fear"),
    };
    format!(
        "**{name}** · {state} · เปลี่ยน {}",
        ui::ts(cycle.expiry, 'R')
    )
}

pub fn bounty_end(app: &App, snapshot: Option<&Snapshot>, now: i64) -> Option<i64> {
    let known = snapshot
        .and_then(|s| s.bounty_end)
        .or_else(|| app.warframe.last_bounty_end())?;
    Some(cycles::extrapolate_bounty(known, now))
}

pub fn all_cycles(app: &App, snapshot: Option<&Snapshot>, now: i64) -> Vec<Cycle> {
    let mut list = vec![cycles::earth(now)];
    if let Some(bounty) = bounty_end(app, snapshot, now) {
        let cetus = cycles::cetus(bounty, now);
        list.push(cetus);
        list.push(cycles::vallis(now));
        list.push(cycles::cambion(&cetus));
        list.push(cycles::zariman(bounty));
    } else {
        list.push(cycles::vallis(now));
    }
    list.push(cycles::duviri(now));
    list
}

pub fn render(app: &App, snapshot: Option<&Snapshot>, now: i64) -> Vec<Component> {
    let cycle_lines: Vec<String> = all_cycles(app, snapshot, now)
        .iter()
        .map(cycle_line)
        .collect();

    let steel = cycles::steel_path(now);
    let next_steel = cycles::steel_path(steel.expiry);
    let mut weekly = vec![format!(
        "**Steel Path** · {}{} · สัปดาห์หน้า: {}",
        steel.reward,
        if steel.umbra { " (สัปดาห์ Umbra)" } else { "" },
        next_steel.reward
    )];
    match snapshot.and_then(|s| s.archon.as_ref()) {
        Some(archon) => weekly.push(format!(
            "**Archon Hunt** · {}{}",
            archon.boss,
            archon
                .shard
                .map(|s| format!(" — {s} Shard"))
                .unwrap_or_default()
        )),
        None => {
            let (boss, shard) = worldstate::archon_fallback(cycles::week_index(now));
            weekly.push(format!(
                "**Archon Hunt** · {boss} — {shard} Shard (คาดการณ์)"
            ));
        }
    }
    if let Some(circuit) = snapshot.and_then(|s| s.circuit.as_ref()) {
        weekly.push(format!(
            "**Circuit** · {}",
            circuit.incarnons.join(", ")
        ));
    }
    if let Some(baro) = snapshot.and_then(|s| s.baro.as_ref()) {
        if baro.activation <= now {
            weekly.push(format!(
                "**Baro Ki'Teer** · อยู่ที่ {} · กลับ {}",
                baro.relay,
                ui::ts(baro.expiry, 'R')
            ));
        } else {
            weekly.push(format!(
                "**Baro Ki'Teer** · มาถึง {} ที่ {}",
                ui::ts(baro.activation, 'R'),
                baro.relay
            ));
        }
    }
    if let Some(sortie) = snapshot.and_then(|s| s.sortie.as_ref()) {
        let faction = if sortie.faction.is_empty() {
            String::new()
        } else {
            format!(" ({})", sortie.faction)
        };
        weekly.push(format!(
            "**Sortie** · {}{faction} · รีเซ็ต {}",
            sortie.boss,
            ui::ts(sortie.expiry, 'R')
        ));
    }
    if let Some(nightwave) = snapshot.and_then(|s| s.nightwave.as_ref()) {
        weekly.push(format!(
            "**Nightwave** · Season {} · จบ {}",
            nightwave.season,
            ui::ts(nightwave.expiry, 'D')
        ));
    }

    let mut card = Card::new(color::BRAND).text(format!(
        "## Warframe · สถานะโลก (PC)\n{}",
        cycle_lines.join("\n")
    ));
    card = card.divider().text(format!(
        "### รายสัปดาห์ · รีเซ็ต {}\n{}",
        ui::ts(steel.expiry, 'R'),
        weekly.join("\n")
    ));
    let stale = app
        .warframe
        .fetched_at()
        .is_none_or(|at| now - at > STALE_AFTER_MS);
    if stale {
        let note = match app.warframe.fetched_at() {
            Some(at) => format!("ข้อมูลอาจไม่อัปเดต (ดึงได้ล่าสุด {})", ui::ts(at, 'R')),
            None => "ข้อมูลอาจไม่อัปเดต (ยังดึงข้อมูลจากเกมไม่ได้)".to_owned(),
        };
        card = card.small(note);
    }
    vec![
        card.small("เวลาตามเขตเวลาของคุณ · ข้อมูลจาก DE")
            .buttons([
                ui::button_with(
                    ButtonStyle::Secondary,
                    format!("{PREFIX}circuit"),
                    "Circuit",
                    false,
                ),
                ui::button_with(
                    ButtonStyle::Secondary,
                    format!("{PREFIX}steel"),
                    "Steel Path",
                    false,
                ),
                ui::button_with(
                    ButtonStyle::Secondary,
                    format!("{PREFIX}missions"),
                    "Sortie / Archon",
                    false,
                ),
            ])
            .build(),
    ]
}

fn digest(components: &[Component]) -> u64 {
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(components)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

/// Guilds not in `checked` are edited even if unchanged: a card deleted while offline sent no
/// event, and the edit's `UNKNOWN_MESSAGE` brings it back.
pub async fn update_all(
    app: &Arc<App>,
    checked: &mut HashSet<Id<GuildMarker>>,
) -> Vec<(Id<GuildMarker>, anyhow::Error)> {
    let snapshot = app.warframe.snapshot();
    let now = jiff::Timestamp::now().as_millisecond();
    let components = render(app, snapshot.as_deref(), now);
    let digest = digest(&components);
    let mut failed = Vec::new();
    for (guild, channel) in app
        .guilds
        .with_channel(ChannelKind::Status)
    {
        if app.shutdown.is_cancelled() {
            break;
        }
        let force = !checked.contains(&guild);
        match upsert(app, guild, channel, &components, digest, force).await {
            Ok(()) => {
                checked.insert(guild);
            }
            Err(error) => failed.push((guild, error)),
        }
    }
    failed
}

pub async fn refresh_guild(app: &Arc<App>, guild: Id<GuildMarker>, force: bool) {
    let Some(channel) = app
        .guild(guild)
        .channel(ChannelKind::Status)
    else {
        let _guard = app.bot_messages.lock().await;
        if let Ok(Some(old)) = app
            .db
            .call(move |c| db::messages::get(c, guild, BotMessageKind::Status))
            .await
        {
            let _ = app
                .http
                .delete_message(old.channel_id, old.message_id)
                .await;
            let _ = app
                .db
                .call(move |c| db::messages::delete(c, guild, BotMessageKind::Status))
                .await;
        }
        return;
    };
    let snapshot = app.warframe.snapshot();
    let components = render(
        app,
        snapshot.as_deref(),
        jiff::Timestamp::now().as_millisecond(),
    );
    let digest = digest(&components);
    if let Err(error) = upsert(app, guild, channel, &components, digest, force).await {
        tracing::warn!(guild = guild.get(), %error, "status refresh failed");
    }
}

async fn upsert(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    channel: Id<twilight_model::id::marker::ChannelMarker>,
    components: &[Component],
    digest: u64,
    force: bool,
) -> anyhow::Result<()> {
    let _guard = app.bot_messages.lock().await;
    let stored = app
        .db
        .call(move |c| db::messages::get(c, guild, BotMessageKind::Status))
        .await?;
    if let Some(old) = stored {
        if old.channel_id == channel && old.digest == Some(digest) && !force {
            return Ok(());
        }
        if old.channel_id == channel && old.digest.is_some() {
            match app
                .http
                .update_message(channel, old.message_id)
                .components(Some(components))
                .await
            {
                Ok(_) => {
                    let record = BotMessage { digest: Some(digest), ..old };
                    app.db
                        .call(move |c| db::messages::set(c, guild, BotMessageKind::Status, record))
                        .await?;
                    metrics::counter!("status_updates_total", "op" => "edit").increment(1);
                    return Ok(());
                }
                Err(error) if errors::is(&error, errors::UNKNOWN_MESSAGE) => {}
                Err(error) => return Err(error.into()),
            }
        } else if let Err(error) = app
            .http
            .delete_message(old.channel_id, old.message_id)
            .await
            && !errors::is(&error, errors::UNKNOWN_MESSAGE)
        {
            tracing::debug!(%error, "could not delete previous status message");
        }
    }
    let message = crate::discord::post_card(app, guild, "status", channel, components).await?;
    let record = BotMessage { channel_id: channel, message_id: message.id, digest: Some(digest) };
    app.db
        .call(move |c| db::messages::set(c, guild, BotMessageKind::Status, record))
        .await?;
    metrics::counter!("status_updates_total", "op" => "create").increment(1);
    Ok(())
}

pub async fn on_deleted(app: &Arc<App>, guild: Id<GuildMarker>, ids: &[Id<MessageMarker>]) {
    // Skip the lock unless the stored card was deleted. Mid-post the new ID isn't saved yet, so
    // then always wait for the lock.
    let posting = app.bot_messages.try_lock().is_err();
    if !posting && !db::messages::stored_among(&app.db, guild, BotMessageKind::Status, ids).await {
        return;
    }
    let forgotten = {
        // Don't let a stuck card post hold every delete event; the next update finds a missed card.
        let Ok(_guard) = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            app.bot_messages.lock(),
        )
        .await
        else {
            tracing::warn!(
                guild = guild.get(),
                "card lock busy; skipping a delete check"
            );
            return;
        };
        match app
            .db
            .call(move |c| db::messages::get(c, guild, BotMessageKind::Status))
            .await
        {
            Ok(Some(record)) if ids.contains(&record.message_id) => app
                .db
                .call(move |c| db::messages::delete(c, guild, BotMessageKind::Status))
                .await
                .is_ok(),
            _ => false,
        }
    };
    if forgotten {
        refresh_guild(app, guild, true).await;
    }
}

pub fn steel_card(now: i64) -> Vec<Component> {
    let current = cycles::steel_path(now);
    let mut lines = vec![format!(
        "**ตอนนี้** · {} ({} Steel Essence) · หมด {}",
        current.reward,
        current.cost,
        ui::ts(current.expiry, 'R')
    )];
    for week in cycles::steel_path_upcoming(now, 7) {
        lines.push(format!(
            "{} · {} ({})",
            ui::ts(week.activation, 'd'),
            week.reward,
            week.cost
        ));
    }
    let umbra = cycles::next_umbra_week(if current.umbra { current.expiry } else { now });
    vec![
        Card::new(color::BRAND)
            .title("Steel Path · ร้าน Teshin")
            .text(lines.join("\n"))
            .small(format!(
                "Umbra Forma รอบถัดไป {} (ทุก 8 สัปดาห์)",
                ui::ts(umbra.activation, 'D')
            ))
            .build(),
    ]
}

pub fn missions_card(snapshot: Option<&Snapshot>) -> Vec<Component> {
    let mut card = Card::new(color::BRAND).title("Sortie & Archon Hunt");
    match snapshot.and_then(|s| s.sortie.as_ref()) {
        Some(sortie) => {
            let lines: Vec<String> = sortie
                .missions
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    format!(
                        "{}. **{}** · {}{}",
                        i + 1,
                        m.kind,
                        m.node,
                        m.modifier
                            .as_ref()
                            .map(|x| format!(" — {x}"))
                            .unwrap_or_default()
                    )
                })
                .collect();
            card = card.text(format!(
                "**Sortie** · {} · รีเซ็ต {}\n{}",
                sortie.boss,
                ui::ts(sortie.expiry, 'R'),
                lines.join("\n")
            ));
        }
        None => card = card.text("**Sortie** · ไม่มีข้อมูล"),
    }
    match snapshot.and_then(|s| s.archon.as_ref()) {
        Some(archon) => {
            let lines: Vec<String> = archon
                .missions
                .iter()
                .enumerate()
                .map(|(i, m)| format!("{}. **{}** · {}", i + 1, m.kind, m.node))
                .collect();
            card = card.divider().text(format!(
                "**Archon Hunt** · {}{} · รีเซ็ต {}\n{}",
                archon.boss,
                archon
                    .shard
                    .map(|s| format!(" — {s} Shard"))
                    .unwrap_or_default(),
                ui::ts(archon.expiry, 'R'),
                lines.join("\n")
            ));
        }
        None => {
            card = card
                .divider()
                .text("**Archon Hunt** · ไม่มีข้อมูล")
        }
    }
    vec![card.build()]
}

pub async fn on_button(
    app: &Arc<App>,
    interaction: &Interaction,
    custom_id: &str,
) -> anyhow::Result<()> {
    let snapshot = app.warframe.snapshot();
    let now = jiff::Timestamp::now().as_millisecond();
    let components = match custom_id.strip_prefix(PREFIX) {
        Some("circuit") => circuit::card(
            snapshot
                .as_ref()
                .and_then(|s| s.circuit.as_ref()),
        ),
        Some("steel") => steel_card(now),
        Some("missions") => missions_card(snapshot.as_deref()),
        _ => return Ok(()),
    };
    reply::respond(app, interaction, components, true).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, discord::ui::validate_message, warframe::worldstate};

    const FIXTURE: &str = include_str!("../../tests/fixtures/worldstate.json");
    const NOW: i64 = 1_794_106_985_000;

    #[tokio::test]
    async fn status_and_detail_cards_fit_limits_with_and_without_data() {
        let app = test_app().await;
        let empty = render(&app, None, NOW);
        validate_message(&empty).unwrap();
        let snapshot = worldstate::parse(FIXTURE.as_bytes(), NOW).unwrap();
        app.warframe
            .store(snapshot.clone(), NOW);
        let full = render(&app, Some(&snapshot), NOW);
        validate_message(&full).unwrap();
        let text = serde_json::to_string(&full).unwrap();
        assert!(
            text.contains("Kronia Relay")
                && text.contains("Azure")
                && !text.contains("ข้อมูลอาจไม่อัปเดต")
        );
        assert_eq!(
            digest(&full),
            digest(&render(&app, Some(&snapshot), NOW + 1_000))
        );
        validate_message(&steel_card(NOW)).unwrap();
        validate_message(&missions_card(Some(&snapshot))).unwrap();
        validate_message(&missions_card(None)).unwrap();
        validate_message(&circuit::card(snapshot.circuit.as_ref())).unwrap();
        validate_message(&circuit::card(None)).unwrap();
    }

    #[tokio::test]
    async fn stale_snapshot_is_flagged_and_cycles_extrapolate() {
        let app = test_app().await;
        let snapshot = worldstate::parse(FIXTURE.as_bytes(), NOW).unwrap();
        app.warframe
            .store(snapshot.clone(), NOW);
        let later = NOW + 6 * 60 * 60 * 1000;
        let text = serde_json::to_string(&render(&app, None, later)).unwrap();
        assert!(text.contains("ข้อมูลอาจไม่อัปเดต"));
        assert_eq!(all_cycles(&app, None, later).len(), 6);
    }
}
