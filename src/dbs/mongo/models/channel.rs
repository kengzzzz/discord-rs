use mongodb::bson::{doc, oid::ObjectId};
use serde::{Deserialize, Serialize};
use twilight_interactions::command::{CommandOption, CreateOption};

#[derive(
    CreateOption, CommandOption, Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum ChannelEnum {
    #[option(name = "Notification Channel", value = "notification")]
    Notification,

    #[option(name = "Status Channel", value = "status")]
    Status,

    #[option(name = "Introduction Channel", value = "introduction")]
    Introduction,

    #[option(name = "UpdateRole Channel", value = "update_role")]
    UpdateRole,

    #[option(name = "Broadcast Channel", value = "broadcast")]
    Broadcast,

    #[option(name = "Broadcast Group 1", value = "broadcast_b1")]
    BroadcastB1,

    #[option(name = "Broadcast Group 2", value = "broadcast_b2")]
    BroadcastB2,

    #[option(name = "Quarantine Channel", value = "quarantine")]
    Quarantine,

    #[option(name = "Regular Channel", value = "regular")]
    #[default]
    None,
}

impl ChannelEnum {
    pub fn is_broadcast(self) -> bool {
        matches!(
            self,
            ChannelEnum::Broadcast | ChannelEnum::BroadcastB1 | ChannelEnum::BroadcastB2
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Channel {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    pub channel_type: ChannelEnum,
    pub channel_id: u64,
    pub guild_id: u64,
}
