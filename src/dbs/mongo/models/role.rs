use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};
use twilight_interactions::command::{CommandOption, CreateOption};

#[derive(
    CreateOption, CommandOption, Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum RoleEnum {
    #[option(name = "Guest", value = "guest")]
    Guest,

    #[option(name = "Member", value = "member")]
    Member,

    #[option(name = "Helminth", value = "helminth")]
    Helminth,

    #[option(name = "Riven silver", value = "riven_silver")]
    RivenSilver,

    #[option(name = "Umbral forma", value = "umbral_forma")]
    UmbralForma,

    #[option(name = "Eidolon", value = "eidolon")]
    Eidolon,

    #[option(name = "Live", value = "live")]
    Live,

    #[option(name = "Quarantine", value = "quarantine")]
    Quarantine,

    #[option(name = "Regular", value = "regular")]
    #[default]
    None,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Role {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    pub role_type: RoleEnum,
    pub role_id: u64,
    pub guild_id: u64,
    pub self_assignable: bool,
}
