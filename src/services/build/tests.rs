use super::*;

pub(crate) async fn set_items(items: Vec<ItemEntry>) {
    *ITEMS.write().await = items;
}
