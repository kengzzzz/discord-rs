use super::super::MarketKind;
use super::*;

fn build_session(rank: u8, orders: BTreeMap<u8, Vec<OrderInfo>>) -> MarketSession {
    MarketSession {
        item: "item".to_string(),
        url: "url".to_string(),
        kind: MarketKind::Buy,
        orders,
        rank,
        page: 1,
        max_rank: None,
        last_used: Instant::now(),
        expire_token: CancellationToken::new(),
    }
}

#[test]
fn test_lpage() {
    let mut map = BTreeMap::new();
    let entries: Vec<OrderInfo> = (0..7)
        .map(|i| OrderInfo { quantity: i, platinum: i, ign: format!("u{i}") })
        .collect();
    map.insert(2, entries);

    let session = build_session(2, map);
    assert_eq!(session.lpage(), 2);

    let session = build_session(1, BTreeMap::new());
    assert_eq!(session.lpage(), 1);
}

#[test]
fn test_slice() {
    let mut map = BTreeMap::new();
    let entries: Vec<OrderInfo> = (0..6)
        .map(|i| OrderInfo { quantity: i, platinum: i, ign: format!("u{i}") })
        .collect();
    map.insert(0, entries);

    let mut session = build_session(0, map);

    session.page = 1;
    assert_eq!(session.slice().len(), 5);

    session.page = 2;
    assert_eq!(session.slice().len(), 1);

    session.page = 3;
    assert!(session.slice().is_empty());
}

#[test]
fn test_touch() {
    let mut session = build_session(0, BTreeMap::new());
    let old_token = session.expire_token.clone();
    let old_time = session.last_used;

    session.touch();

    assert!(old_token.is_cancelled());
    assert!(!session.expire_token.is_cancelled());
    assert!(session.last_used > old_time);
}
