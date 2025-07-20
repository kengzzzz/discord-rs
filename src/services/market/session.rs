use std::{collections::BTreeMap, time::Instant};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct OrderInfo {
    pub quantity: u32,
    pub platinum: u32,
    pub ign: String,
}

#[derive(Clone)]
pub struct MarketSession {
    pub item: String,
    pub url: String,
    pub kind: super::MarketKind,
    pub orders: BTreeMap<u8, Vec<OrderInfo>>,
    pub rank: u8,
    pub page: usize,
    pub max_rank: Option<u8>,
    pub last_used: Instant,
    pub expire_token: CancellationToken,
}

impl MarketSession {
    pub fn lpage(&self) -> usize {
        self.orders
            .get(&self.rank)
            .map(|v| v.len().div_ceil(5))
            .unwrap_or(1)
    }

    pub fn slice(&self) -> &[OrderInfo] {
        let start = (self.page.saturating_sub(1)) * 5;
        let orders = self
            .orders
            .get(&self.rank)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let end = start + 5;
        if start >= orders.len() {
            &[]
        } else if end > orders.len() {
            &orders[start..]
        } else {
            &orders[start..end]
        }
    }

    pub fn touch(&mut self) {
        self.last_used = Instant::now();
        self.expire_token.cancel();
        self.expire_token = CancellationToken::new();
    }
}

#[cfg(test)]
#[path = "tests/session.rs"]
mod tests;
