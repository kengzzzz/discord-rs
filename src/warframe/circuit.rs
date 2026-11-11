use twilight_model::channel::message::Component;

use super::worldstate::Circuit;
use crate::discord::ui::{self, Card, color};

const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

const ROTATIONS: [[&str; 5]; 9] = [
    ["Braton", "Lato", "Skana", "Paris", "Kunai"],
    ["Boar", "Gammacor", "Angstrum", "Gorgon", "Anku"],
    ["Bo", "Latron", "Furis", "Furax", "Strun"],
    ["Lex", "Magistar", "Boltor", "Bronco", "Ceramic Dagger"],
    ["Torid", "Dual Toxocyst", "Dual Ichor", "Miter", "Atomos"],
    ["Ack & Brunt", "Soma", "Vasto", "Nami Solo", "Burston"],
    ["Zylok", "Sibear", "Dread", "Despair", "Hate"],
    ["Dera", "Sybaris", "Cestra", "Sicarus", "Okina"],
    ["Vectis", "Stug", "Ballistica", "Destreza", "Obex"],
];

pub fn rotation_index(incarnons: &[String]) -> Option<usize> {
    ROTATIONS.iter().position(|set| {
        incarnons.len() == set.len()
            && set.iter().all(|weapon| {
                incarnons
                    .iter()
                    .any(|live| live == weapon)
            })
    })
}

pub fn upcoming(circuit: &Circuit, weeks: usize) -> Vec<(i64, &'static [&'static str; 5])> {
    let Some(index) = rotation_index(&circuit.incarnons) else {
        return Vec::new();
    };
    (1..=weeks.min(ROTATIONS.len() - 1))
        .map(|offset| {
            (
                circuit.expiry + (offset as i64 - 1) * WEEK_MS,
                &ROTATIONS[(index + offset) % ROTATIONS.len()],
            )
        })
        .collect()
}

pub fn wiki_links<S: AsRef<str>>(names: &[S]) -> String {
    names
        .iter()
        .map(|name| {
            let name = name.as_ref();
            format!(
                "[{name}](https://wiki.warframe.com/w/{})",
                name.replace(' ', "_")
                    .replace('&', "%26")
            )
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

pub fn card(circuit: Option<&Circuit>) -> Vec<Component> {
    let Some(circuit) = circuit else {
        return ui::notice(
            color::WARNING,
            "The Circuit",
            "ยังดึงข้อมูล Circuit ไม่ได้ ลองใหม่ภายหลัง",
        );
    };
    let mut card = Card::new(color::BRAND)
        .title("The Circuit")
        .text(format!(
            "**Incarnon (Steel Path)**\n{}",
            wiki_links(&circuit.incarnons)
        ));
    if !circuit.warframes.is_empty() {
        card = card.text(format!(
            "**Warframe (Normal)**\n{}",
            wiki_links(&circuit.warframes)
        ));
    }
    card = card
        .small(format!("รีเซ็ต {}", ui::ts(circuit.expiry, 'R')))
        .divider();
    let next = upcoming(circuit, ROTATIONS.len() - 1);
    card = if next.is_empty() {
        card.text("ยังไม่ทราบ Incarnon สัปดาห์ถัดไป")
    } else {
        let lines: Vec<String> = next
            .iter()
            .map(|(starts, set)| format!("{} · {}", ui::ts(*starts, 'd'), set.join(", ")))
            .collect();
        card.text(format!(
            "**Incarnon สัปดาห์ถัดไป**\n{}",
            lines.join("\n")
        ))
    };
    vec![
        card.small("ลำดับตาม Update 43 ของ DE")
            .buttons([ui::link("https://wiki.warframe.com/w/The_Circuit", "Wiki")])
            .build(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circuit(weapons: &[&str]) -> Circuit {
        Circuit {
            activation: 0,
            expiry: 1_000,
            incarnons: weapons
                .iter()
                .map(|w| (*w).to_owned())
                .collect(),
            warframes: Vec::new(),
        }
    }

    #[test]
    fn upcoming_rotation_follows_live_set_order_independent() {
        let live = circuit(&["Hate", "Zylok", "Despair", "Sibear", "Dread"]);
        assert_eq!(rotation_index(&live.incarnons), Some(6));
        let next = upcoming(&live, 2);
        assert_eq!(next[0], (1_000, &ROTATIONS[7]));
        assert_eq!(next[1].0, 1_000 + WEEK_MS);
        assert!(upcoming(&circuit(&["Unknown"]), 3).is_empty());
    }

    #[test]
    fn wiki_links_escape_ampersand() {
        assert_eq!(
            wiki_links(&["Ack & Brunt"]),
            "[Ack & Brunt](https://wiki.warframe.com/w/Ack_%26_Brunt)"
        );
    }
}
