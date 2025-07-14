use super::super::models::ChatEntry;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use tokio::sync::RwLock;

pub(super) static HISTORY_STORE: Lazy<RwLock<HashMap<u64, Vec<ChatEntry>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
pub(super) static PROMPT_STORE: Lazy<RwLock<HashMap<u64, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
