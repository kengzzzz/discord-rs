use std::collections::BTreeMap;

use crate::services::market::{MarketKind, MarketService, MarketSession, OrderInfo};

#[test]
fn test_market_components_pagination() {
    let mut orders = BTreeMap::new();
    let list: Vec<_> = (0..10)
        .map(|i| OrderInfo {
            quantity: 1,
            platinum: i,
            ign: format!("U{i}"),
        })
        .collect();
    orders.insert(0, list);
    let session = MarketSession {
        item: "Item".into(),
        url: "url".into(),
        kind: MarketKind::Buy,
        orders,
        rank: 0,
        page: 1,
        max_rank: None,
        last_used: std::time::Instant::now(),
        expire_token: tokio_util::sync::CancellationToken::new(),
    };
    let comps = MarketService::components(&session);
    assert_eq!(comps.len(), 1);
    let row = match &comps[0] {
        twilight_model::channel::message::Component::ActionRow(r) => r,
        _ => panic!("expected action row"),
    };
    assert_eq!(row.components.len(), 3); // prev, next, refresh
    if let twilight_model::channel::message::Component::Button(btn) = &row.components[0] {
        assert!(btn.disabled);
    } else {
        panic!("expected button");
    }
    if let twilight_model::channel::message::Component::Button(btn) = &row.components[1] {
        assert!(!btn.disabled);
    } else {
        panic!("expected button");
    }
}

#[test]
fn test_market_components_rank_buttons() {
    let mut orders = BTreeMap::new();
    orders.insert(
        0,
        vec![OrderInfo {
            quantity: 1,
            platinum: 1,
            ign: "A".into(),
        }],
    );
    let session = MarketSession {
        item: "Item".into(),
        url: "url".into(),
        kind: MarketKind::Sell,
        orders,
        rank: 0,
        page: 1,
        max_rank: Some(2),
        last_used: std::time::Instant::now(),
        expire_token: tokio_util::sync::CancellationToken::new(),
    };
    let comps = MarketService::components(&session);
    let row = match &comps[0] {
        twilight_model::channel::message::Component::ActionRow(r) => r,
        _ => panic!("expected action row"),
    };
    assert_eq!(row.components.len(), 5); // prev, next, next_rank, prev_rank, refresh
    if let twilight_model::channel::message::Component::Button(btn) = &row.components[2] {
        assert!(!btn.disabled);
    } else {
        panic!("expected button");
    }
    if let twilight_model::channel::message::Component::Button(btn) = &row.components[3] {
        assert!(btn.disabled); // rank == 0
    } else {
        panic!("expected button");
    }
}
