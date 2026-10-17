use super::*;

fn rotation_data(index: usize) -> CircuitData {
    CircuitData {
        activation: "2026-10-12T00:00:00Z".parse().unwrap(),
        expiry: "2026-10-19T00:00:00Z".parse().unwrap(),
        weapons: ROTATIONS[index]
            .iter()
            .map(|weapon| (*weapon).to_owned())
            .collect(),
        warframes: ["Nekros", "Valkyr", "Oberon"]
            .map(String::from)
            .to_vec(),
    }
}

fn upcoming(description: &str) -> Vec<&str> {
    description
        .split("**Next Incarnons**\n")
        .nth(1)
        .unwrap()
        .lines()
        .collect()
}

#[test]
fn status_embed_shows_both_categories_and_next_three_weeks() {
    let mut data = rotation_data(6);
    data.weapons.reverse();
    let embed = status_embed(&data).unwrap();

    assert_eq!(embed.title.as_deref(), Some("Circuit Rotation"));
    assert_eq!(embed.color, Some(COLOR));
    assert!(
        embed
            .footer
            .unwrap()
            .text
            .contains("/warframe circuit")
    );
    let description = embed.description.unwrap();
    assert!(
        description
            .starts_with("**Incarnons (Steel Path)**\n[Hate](https://wiki.warframe.com/w/Hate)")
    );
    assert!(
        description
            .contains("**Warframes (Normal)**\n[Nekros](https://wiki.warframe.com/w/Nekros), ")
    );
    let rows = upcoming(&description);
    assert_eq!(
        rows,
        [
            "<t:1792368000:R> · Dera, Sybaris, Cestra, Sicarus, Okina",
            "<t:1792972800:d> · Vectis, Stug, Ballistica, Destreza, Obex",
            "<t:1793577600:d> · Braton, Lato, Skana, Paris, Kunai",
        ]
    );
}

#[test]
fn schedule_embed_lists_every_other_week_without_repeating_the_active_one() {
    let data = rotation_data(6);
    let description = schedule_embed(&data)
        .unwrap()
        .description
        .unwrap();
    let rows = upcoming(&description);

    assert_eq!(rows.len(), ROTATIONS.len() - 1);
    let last = (data.expiry + Duration::weeks(7)).timestamp();
    assert_eq!(
        rows[7],
        format!("<t:{last}:d> · Ack & Brunt, Soma, Vasto, Nami Solo, Burston")
    );
    assert!(
        rows.iter()
            .all(|row| !row.contains("Zylok"))
    );
}

#[test]
fn circuit_embeds_wrap_from_week_i_and_fit_discord_limits() {
    for index in 0..ROTATIONS.len() {
        // build_embed validates the entire embed, including Discord's description limit.
        let embed = schedule_embed(&rotation_data(index)).unwrap();
        status_embed(&rotation_data(index)).unwrap();
        if index == 8 {
            assert!(
                upcoming(&embed.description.unwrap())[0]
                    .ends_with("Braton, Lato, Skana, Paris, Kunai")
            );
        }
    }
}

#[test]
fn circuit_keeps_unknown_live_choices_without_guessing_upcoming_weeks() {
    let mut data = rotation_data(6);
    data.weapons[0] = "New Weapon".to_owned();
    data.warframes.clear();
    let description = status_embed(&data)
        .unwrap()
        .description
        .unwrap();

    assert!(description.contains("[New Weapon](https://wiki.warframe.com/w/New_Weapon)"));
    assert!(!description.contains("Warframes (Normal)"));
    assert!(description.contains("**Resets** <t:1792368000:R>"));
    assert!(description.contains("Upcoming Incarnons are unknown"));
    assert!(!description.contains("Next Incarnons"));
}
