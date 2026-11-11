use std::{collections::HashMap, sync::LazyLock};

use serde::{Deserialize, Deserializer, de::IgnoredAny};

pub const URL: &str = "https://api.warframe.com/cdn/worldState.php";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub generated_at: i64,
    pub bounty_end: Option<i64>,
    pub circuit: Option<Circuit>,
    pub archon: Option<Archon>,
    pub sortie: Option<Sortie>,
    pub baro: Option<Baro>,
    pub nightwave: Option<Nightwave>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Circuit {
    pub activation: i64,
    pub expiry: i64,
    pub incarnons: Vec<String>,
    pub warframes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mission {
    pub kind: String,
    pub node: String,
    pub modifier: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Archon {
    pub activation: i64,
    pub expiry: i64,
    pub boss: String,
    pub shard: Option<&'static str>,
    pub missions: Vec<Mission>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sortie {
    pub activation: i64,
    pub expiry: i64,
    pub boss: String,
    pub faction: String,
    pub missions: Vec<Mission>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Baro {
    pub activation: i64,
    pub expiry: i64,
    pub relay: String,
    pub item_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nightwave {
    pub season: u32,
    pub expiry: i64,
    pub daily: usize,
    pub weekly: usize,
    pub elite: usize,
}

static NODES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/worldstate/nodes.json")).unwrap_or_default()
});
static MISSION_TYPES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../assets/worldstate/mission_types.json"
    ))
    .unwrap_or_default()
});

#[derive(Deserialize, Default)]
struct SortieData {
    bosses: HashMap<String, SortieBoss>,
    modifiers: HashMap<String, String>,
}

#[derive(Deserialize)]
struct SortieBoss {
    name: String,
    faction: String,
}

static SORTIE_DATA: LazyLock<SortieData> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../assets/worldstate/sortie.json"
    ))
    .unwrap_or_default()
});

pub fn node_name(key: &str) -> String {
    NODES
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.to_owned())
}

pub fn mission_name(key: &str) -> String {
    MISSION_TYPES
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.to_owned())
}

pub fn relay_name(key: &str) -> String {
    let name = match key {
        "MercuryHUB" => "Larunda Relay (Mercury)",
        "VenusHUB" => "Vesper Relay (Venus)",
        "EarthHUB" => "Strata Relay (Earth)",
        "SaturnHUB" => "Kronia Relay (Saturn)",
        "EuropaHUB" => "Leonov Relay (Europa)",
        "PlutoHUB" => "Orcus Relay (Pluto)",
        "ErisHUB" => "Kuiper Relay (Eris)",
        "TradeHUB1" => "Maroo's Bazaar (Mars)",
        other => return node_name(other),
    };
    name.to_owned()
}

fn archon_shard(boss: &str) -> (String, Option<&'static str>) {
    match boss {
        "SORTIE_BOSS_AMAR" => ("Archon Amar".into(), Some("Crimson")),
        "SORTIE_BOSS_NIRA" => ("Archon Nira".into(), Some("Amber")),
        "SORTIE_BOSS_BOREAL" => ("Archon Boreal".into(), Some("Azure")),
        other => {
            let name = SORTIE_DATA
                .bosses
                .get(other)
                .map_or_else(|| other.to_owned(), |b| b.name.clone());
            (name, None)
        }
    }
}

pub fn archon_fallback(week: i64) -> (&'static str, &'static str) {
    [("Archon Amar", "Crimson"), ("Archon Nira", "Amber"), ("Archon Boreal", "Azure")]
        [week.rem_euclid(3) as usize]
}

fn date<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(rename = "$date")]
        date: Inner,
    }
    #[derive(Deserialize)]
    struct Inner {
        #[serde(rename = "$numberLong")]
        value: String,
    }
    let wrapper = Wrapper::deserialize(deserializer)?;
    wrapper
        .date
        .value
        .parse()
        .map_err(serde::de::Error::custom)
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Raw {
    #[serde(default)]
    time: i64,
    #[serde(default)]
    syndicate_missions: Vec<RawSyndicate>,
    #[serde(default, rename = "EndlessXpSchedule")]
    endless_xp: Vec<RawCircuit>,
    #[serde(default)]
    lite_sorties: Vec<RawSortie>,
    #[serde(default)]
    sorties: Vec<RawSortie>,
    #[serde(default)]
    void_traders: Vec<RawTrader>,
    #[serde(default)]
    season_info: Option<RawSeason>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawSyndicate {
    tag: String,
    #[serde(deserialize_with = "date")]
    activation: i64,
    #[serde(deserialize_with = "date")]
    expiry: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawCircuit {
    #[serde(deserialize_with = "date")]
    activation: i64,
    #[serde(deserialize_with = "date")]
    expiry: i64,
    #[serde(default)]
    category_choices: Vec<RawChoice>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawChoice {
    category: String,
    #[serde(default)]
    choices: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawSortie {
    #[serde(deserialize_with = "date")]
    activation: i64,
    #[serde(deserialize_with = "date")]
    expiry: i64,
    #[serde(default)]
    boss: String,
    #[serde(default)]
    missions: Vec<RawMission>,
    #[serde(default)]
    variants: Vec<RawMission>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMission {
    #[serde(default)]
    mission_type: String,
    #[serde(default)]
    node: String,
    #[serde(default)]
    modifier_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawTrader {
    #[serde(deserialize_with = "date")]
    activation: i64,
    #[serde(deserialize_with = "date")]
    expiry: i64,
    #[serde(default)]
    node: String,
    #[serde(default)]
    manifest: Vec<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawSeason {
    #[serde(deserialize_with = "date")]
    expiry: i64,
    #[serde(default)]
    season: u32,
    #[serde(default)]
    active_challenges: Vec<RawChallenge>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawChallenge {
    #[serde(default)]
    daily: bool,
    #[serde(default)]
    challenge: String,
}

pub fn parse(body: &[u8], now: i64) -> anyhow::Result<Snapshot> {
    let raw: Raw = serde_json::from_slice(body)?;

    let bounty_end = raw
        .syndicate_missions
        .iter()
        .filter(|m| m.tag == "CetusSyndicate")
        .find(|m| m.activation <= now && now < m.expiry)
        .or_else(|| {
            raw.syndicate_missions
                .iter()
                .filter(|m| m.tag == "CetusSyndicate" && m.expiry > now)
                .min_by_key(|m| m.expiry)
        })
        .map(|m| m.expiry);

    let circuit = raw
        .endless_xp
        .into_iter()
        .find(|s| s.activation <= now && now < s.expiry)
        .and_then(|s| {
            let mut incarnons = Vec::new();
            let mut warframes = Vec::new();
            for choice in s.category_choices {
                match choice.category.as_str() {
                    "EXC_HARD" => incarnons = choice.choices,
                    "EXC_NORMAL" => warframes = choice.choices,
                    _ => {}
                }
            }
            (!incarnons.is_empty()).then_some(Circuit {
                activation: s.activation,
                expiry: s.expiry,
                incarnons,
                warframes,
            })
        });

    let missions = |raw: Vec<RawMission>| -> Vec<Mission> {
        raw.into_iter()
            .map(|m| Mission {
                kind: mission_name(&m.mission_type),
                node: node_name(&m.node),
                modifier: m.modifier_type.map(|key| {
                    SORTIE_DATA
                        .modifiers
                        .get(&key)
                        .cloned()
                        .unwrap_or(key)
                }),
            })
            .collect()
    };

    let archon = raw
        .lite_sorties
        .into_iter()
        .find(|s| s.expiry > now)
        .map(|s| {
            let (boss, shard) = archon_shard(&s.boss);
            Archon {
                activation: s.activation,
                expiry: s.expiry,
                boss,
                shard,
                missions: missions(s.missions),
            }
        });

    let sortie = raw
        .sorties
        .into_iter()
        .find(|s| s.expiry > now)
        .map(|s| {
            let (boss, faction) = SORTIE_DATA
                .bosses
                .get(&s.boss)
                .map_or_else(
                    || (s.boss.clone(), String::new()),
                    |b| (b.name.clone(), b.faction.clone()),
                );
            let source = if s.variants.is_empty() { s.missions } else { s.variants };
            Sortie {
                activation: s.activation,
                expiry: s.expiry,
                boss,
                faction,
                missions: missions(source),
            }
        });

    let baro = raw
        .void_traders
        .into_iter()
        .find(|t| t.expiry > now)
        .map(|t| Baro {
            activation: t.activation,
            expiry: t.expiry,
            relay: relay_name(&t.node),
            item_count: t.manifest.len(),
        });

    let nightwave = raw
        .season_info
        .filter(|s| s.expiry > now)
        .map(|s| {
            let elite = s
                .active_challenges
                .iter()
                .filter(|c| {
                    c.challenge
                        .to_ascii_lowercase()
                        .contains("hard")
                })
                .count();
            let daily = s
                .active_challenges
                .iter()
                .filter(|c| c.daily)
                .count();
            Nightwave {
                season: s.season,
                expiry: s.expiry,
                daily,
                weekly: s
                    .active_challenges
                    .len()
                    .saturating_sub(daily + elite),
                elite,
            }
        });

    Ok(Snapshot {
        generated_at: raw.time * 1000,
        bounty_end,
        circuit,
        archon,
        sortie,
        baro,
        nightwave,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/worldstate.json");
    const FIXTURE_NOW: i64 = 1_794_106_985_000;

    #[test]
    fn parses_live_fixture() {
        let snap = parse(FIXTURE.as_bytes(), FIXTURE_NOW).unwrap();
        assert_eq!(snap.generated_at, FIXTURE_NOW);
        assert_eq!(snap.bounty_end, Some(1_794_107_370_299));
        let circuit = snap.circuit.expect("circuit");
        assert_eq!(circuit.incarnons.len(), 5);
        let archon = snap.archon.expect("archon");
        assert_eq!(
            (archon.boss.as_str(), archon.shard),
            ("Archon Boreal", Some("Azure"))
        );
        assert_eq!(archon.missions.len(), 3);
        assert!(
            archon.missions[0]
                .node
                .contains("Earth")
        );
        let sortie = snap.sortie.expect("sortie");
        assert_eq!(sortie.missions.len(), 3);
        assert!(sortie.missions[0].modifier.is_some());
        let baro = snap.baro.expect("baro");
        assert_eq!(baro.relay, "Kronia Relay (Saturn)");
        assert!(baro.item_count > 0);
        let nightwave = snap.nightwave.expect("nightwave");
        assert_eq!(nightwave.season, 18);
        assert_eq!(
            nightwave.daily + nightwave.weekly + nightwave.elite,
            10
        );
    }

    #[test]
    fn picks_active_bounty_window_over_prepublished_one() {
        let snap = parse(FIXTURE.as_bytes(), 1_794_107_370_300).unwrap();
        assert!(snap.bounty_end.unwrap() > 1_794_107_370_300);
    }

    #[test]
    fn unknown_lookups_fall_back_to_raw_keys() {
        assert_eq!(node_name("NoSuchNode"), "NoSuchNode");
        assert_eq!(relay_name("SaturnHUB"), "Kronia Relay (Saturn)");
        assert_eq!(archon_fallback(311), ("Archon Boreal", "Azure"));
    }
}
