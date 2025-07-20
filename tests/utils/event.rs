use twilight_model::gateway::payload::incoming::{MessageDelete, MessageDeleteBulk};
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker},
};

pub fn message_delete(guild_id: u64, channel_id: u64, message_id: u64) -> MessageDelete {
    MessageDelete {
        guild_id: Some(Id::<GuildMarker>::new(guild_id)),
        channel_id: Id::<ChannelMarker>::new(channel_id),
        id: Id::<MessageMarker>::new(message_id),
    }
}

pub fn message_delete_bulk(
    guild_id: u64,
    channel_id: u64,
    message_ids: Vec<u64>,
) -> MessageDeleteBulk {
    MessageDeleteBulk {
        guild_id: Some(Id::<GuildMarker>::new(guild_id)),
        channel_id: Id::<ChannelMarker>::new(channel_id),
        ids: message_ids.into_iter().map(Id::new).collect(),
    }
}
