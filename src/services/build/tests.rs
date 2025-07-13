use super::cache::{ITEMS, ItemEntry};

pub(crate) async fn set_items(items: Vec<ItemEntry>) {
    *ITEMS.write().await = items;
}
