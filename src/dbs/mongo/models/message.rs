use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageEnum {
    #[default]
    Role,
    Status,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Message {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    pub guild_id: u64,
    pub channel_id: u64,
    pub message_id: u64,
    pub message_type: MessageEnum,
}
