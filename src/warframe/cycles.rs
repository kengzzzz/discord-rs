const MINUTE: i64 = 60_000;
const EARTH_HALF_S: i64 = 14_400;
const CETUS_WINDOW: i64 = 9_000_000;
const CETUS_NIGHT: i64 = 3_000_000;
pub const BOUNTY_PERIOD: i64 = 8_998_875;
const ZARIMAN_EPOCH: i64 = 1_655_182_800_000;
const ZARIMAN_LOOP: i64 = 18_000_000;
const VALLIS_EPOCH: i64 = 1_770_234_408_000;
const VALLIS_LOOP: i64 = 1_600_000;
const VALLIS_WARM: i64 = 400_000;
const DUVIRI_OFFSET_S: i64 = 52;
const DUVIRI_MOOD_S: i64 = 7_200;
const DUVIRI_MOODS: [&str; 5] = ["sorrow", "fear", "joy", "anger", "envy"];
const STEEL_EPOCH_S: i64 = 1_605_484_800;
const WEEK_S: i64 = 604_800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleKind {
    Earth,
    Cetus,
    Vallis,
    Cambion,
    Zariman,
    Duviri,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycle {
    pub kind: CycleKind,
    pub state: &'static str,
    pub activation: i64,
    pub expiry: i64,
    pub boundary: i64,
}

fn floor_min(ms: i64) -> i64 {
    ms - ms.rem_euclid(MINUTE)
}

pub fn earth(now: i64) -> Cycle {
    let t = now.div_euclid(1000);
    let day = t.rem_euclid(2 * EARTH_HALF_S) < EARTH_HALF_S;
    let expiry = (t.div_euclid(EARTH_HALF_S) + 1) * EARTH_HALF_S * 1000;
    Cycle {
        kind: CycleKind::Earth,
        state: if day { "day" } else { "night" },
        activation: expiry - EARTH_HALF_S * 1000,
        expiry,
        boundary: expiry,
    }
}

pub fn cetus(bounty_end: i64, now: i64) -> Cycle {
    let end = floor_min(bounty_end);
    let night_start = end - CETUS_NIGHT;
    if now >= night_start {
        Cycle {
            kind: CycleKind::Cetus,
            state: "night",
            activation: night_start,
            expiry: end,
            boundary: bounty_end,
        }
    } else {
        Cycle {
            kind: CycleKind::Cetus,
            state: "day",
            activation: end - CETUS_WINDOW,
            expiry: night_start,
            boundary: bounty_end - CETUS_NIGHT,
        }
    }
}

pub fn cambion(cetus: &Cycle) -> Cycle {
    Cycle {
        kind: CycleKind::Cambion,
        state: if cetus.state == "night" { "vome" } else { "fass" },
        ..*cetus
    }
}

pub fn zariman(bounty_end: i64) -> Cycle {
    let end = floor_min(bounty_end);
    let x = end + bounty_end.rem_euclid(1000) - 5000 - ZARIMAN_EPOCH;
    let corpus = x.rem_euclid(ZARIMAN_LOOP) < ZARIMAN_LOOP / 2;
    Cycle {
        kind: CycleKind::Zariman,
        state: if corpus { "corpus" } else { "grineer" },
        activation: end - CETUS_WINDOW,
        expiry: end,
        boundary: bounty_end,
    }
}

pub fn vallis(now: i64) -> Cycle {
    let p = (now - VALLIS_EPOCH).rem_euclid(VALLIS_LOOP);
    let start = now - p;
    let warm = p < VALLIS_WARM;
    let (activation, expiry) = if warm {
        (start, start + VALLIS_WARM)
    } else {
        (start + VALLIS_WARM, start + VALLIS_LOOP)
    };
    Cycle {
        kind: CycleKind::Vallis,
        state: if warm { "warm" } else { "cold" },
        activation,
        expiry,
        boundary: expiry,
    }
}

pub fn duviri(now: i64) -> Cycle {
    let t = now.div_euclid(1000);
    let c = (t - DUVIRI_OFFSET_S).rem_euclid(DUVIRI_MOOD_S * 5);
    let boundary = (t - c % DUVIRI_MOOD_S + DUVIRI_MOOD_S) * 1000;
    let expiry = floor_min(boundary);
    Cycle {
        kind: CycleKind::Duviri,
        state: DUVIRI_MOODS[(c / DUVIRI_MOOD_S) as usize],
        activation: expiry - DUVIRI_MOOD_S * 1000,
        expiry,
        boundary,
    }
}

pub fn extrapolate_bounty(last_end: i64, now: i64) -> i64 {
    if last_end > now {
        return last_end;
    }
    let periods = (now - last_end) / BOUNTY_PERIOD + 1;
    last_end + periods * BOUNTY_PERIOD
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SteelPath {
    pub reward: &'static str,
    pub cost: u32,
    pub activation: i64,
    pub expiry: i64,
    pub umbra: bool,
}

const STEEL_ROTATION: [(&str, u32); 8] = [
    ("Umbra Forma Blueprint", 150),
    ("50,000 Kuva", 55),
    ("Kitgun Riven Mod", 75),
    ("3x Forma", 75),
    ("Zaw Riven Mod", 75),
    ("30,000 Endo", 150),
    ("Rifle Riven Mod", 75),
    ("Shotgun Riven Mod", 75),
];

pub fn week_index(now: i64) -> i64 {
    (now.div_euclid(1000) - STEEL_EPOCH_S).div_euclid(WEEK_S)
}

pub fn steel_path(now: i64) -> SteelPath {
    let week = week_index(now);
    let index = week.rem_euclid(STEEL_ROTATION.len() as i64) as usize;
    let activation = (STEEL_EPOCH_S + week * WEEK_S) * 1000;
    let (reward, cost) = STEEL_ROTATION[index];
    SteelPath { reward, cost, activation, expiry: activation + WEEK_S * 1000, umbra: index == 0 }
}

pub fn steel_path_upcoming(now: i64, weeks: usize) -> impl Iterator<Item = SteelPath> {
    (1..=weeks as i64).map(move |offset| steel_path(now + offset * WEEK_S * 1000))
}

pub fn next_umbra_week(now: i64) -> SteelPath {
    let mut week = steel_path(now);
    while !week.umbra {
        week = steel_path(week.expiry);
    }
    week
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_794_107_820_000;
    const BOUNTY_END: i64 = 1_794_116_369_174;
    const H0400: i64 = 1_794_110_400_000;

    #[test]
    fn earth_matches_reference() {
        let c = earth(NOW);
        assert_eq!((c.state, c.expiry), ("day", H0400));
    }

    #[test]
    fn cetus_cambion_zariman_match_reference() {
        let c = cetus(BOUNTY_END, NOW);
        assert_eq!(c.state, "day");
        assert_eq!(c.activation, 1_794_116_340_000 - 9_000_000);
        assert_eq!(c.expiry, 1_794_116_340_000 - 3_000_000);
        assert_eq!(c.boundary, BOUNTY_END - 3_000_000);
        let night = cetus(BOUNTY_END, 1_794_116_340_000 - 3_000_000);
        assert_eq!(
            (night.state, night.expiry),
            ("night", 1_794_116_340_000)
        );
        assert_eq!(cambion(&c).state, "fass");
        assert_eq!(cambion(&night).state, "vome");
        let z = zariman(BOUNTY_END);
        assert_eq!(
            (z.state, z.expiry),
            ("grineer", 1_794_116_340_000)
        );
    }

    #[test]
    fn vallis_matches_reference() {
        let c = vallis(NOW);
        assert_eq!(c.state, "cold");
        assert_eq!(c.expiry, H0400 - 40 * 60_000 + 8_000);
        let warm = vallis(c.expiry);
        assert_eq!(
            (warm.state, warm.expiry),
            ("warm", c.expiry + 400_000)
        );
    }

    #[test]
    fn duviri_matches_reference_and_separates_display_from_boundary() {
        let c = duviri(NOW);
        assert_eq!(c.state, "fear");
        assert_eq!(c.expiry, H0400);
        assert_eq!(c.boundary, H0400 + 52_000);
        assert_eq!(duviri(H0400 + 52_000).state, "joy");
    }

    #[test]
    fn steel_path_matches_reference_and_finds_umbra_week() {
        let sp = steel_path(NOW);
        assert_eq!(week_index(NOW), 311);
        assert_eq!(sp.reward, "Shotgun Riven Mod");
        assert_eq!(sp.expiry, 1_794_182_400_000);
        let next = next_umbra_week(NOW);
        assert!(next.umbra);
        assert_eq!(next.activation, 1_794_182_400_000);
        assert_eq!(
            steel_path_upcoming(NOW, 2)
                .map(|w| w.reward)
                .collect::<Vec<_>>(),
            ["Umbra Forma Blueprint", "50,000 Kuva"]
        );
    }

    #[test]
    fn bounty_extrapolation_moves_past_now() {
        assert_eq!(extrapolate_bounty(BOUNTY_END, NOW), BOUNTY_END);
        let later = BOUNTY_END + 3 * BOUNTY_PERIOD + 5;
        let next = extrapolate_bounty(BOUNTY_END, later);
        assert!(next > later && next - later <= BOUNTY_PERIOD);
    }
}
