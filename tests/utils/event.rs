use twilight_model::gateway::payload::incoming::{MessageDelete, MessageDeleteBulk};
use twilight_model::gateway::payload::incoming::{ReactionAdd, ReactionRemove, Ready};
use twilight_model::{
    channel::Message,
    channel::message::{EmojiReactionType, MessageType},
    guild::UnavailableGuild,
    id::{
        Id,
        marker::{ApplicationMarker, ChannelMarker, GuildMarker, MessageMarker},
    },
    oauth::PartialApplication,
    user::{CurrentUser, User},
    util::datetime::Timestamp,
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
        ids: message_ids
            .into_iter()
            .map(Id::new)
            .collect(),
    }
}

pub fn reaction_add(
    guild_id: u64,
    channel_id: u64,
    message_id: u64,
    user_id: u64,
    emoji: &str,
) -> ReactionAdd {
    ReactionAdd(twilight_model::gateway::GatewayReaction {
        burst: false,
        burst_colors: Vec::new(),
        channel_id: Id::new(channel_id),
        emoji: EmojiReactionType::Unicode { name: emoji.to_owned() },
        guild_id: Some(Id::new(guild_id)),
        member: None,
        message_author_id: None,
        message_id: Id::new(message_id),
        user_id: Id::new(user_id),
    })
}

pub fn reaction_remove(
    guild_id: u64,
    channel_id: u64,
    message_id: u64,
    user_id: u64,
    emoji: &str,
) -> ReactionRemove {
    ReactionRemove(twilight_model::gateway::GatewayReaction {
        burst: false,
        burst_colors: Vec::new(),
        channel_id: Id::new(channel_id),
        emoji: EmojiReactionType::Unicode { name: emoji.to_owned() },
        guild_id: Some(Id::new(guild_id)),
        member: None,
        message_author_id: None,
        message_id: Id::new(message_id),
        user_id: Id::new(user_id),
    })
}

pub fn make_message(
    id: u64,
    channel_id: u64,
    guild_id: Option<u64>,
    user_id: u64,
    content: &str,
) -> Message {
    Message {
        activity: None,
        application: None,
        application_id: None,
        attachments: Vec::new(),
        author: User {
            accent_color: None,
            avatar: None,
            avatar_decoration: None,
            avatar_decoration_data: None,
            banner: None,
            bot: false,
            discriminator: 0,
            email: None,
            flags: None,
            global_name: None,
            id: Id::new(user_id),
            locale: None,
            mfa_enabled: None,
            name: "tester".to_owned(),
            premium_type: None,
            public_flags: None,
            system: None,
            verified: None,
        },
        call: None,
        channel_id: Id::new(channel_id),
        components: Vec::new(),
        content: content.to_owned(),
        edited_timestamp: None,
        embeds: Vec::new(),
        flags: Some(twilight_model::channel::message::MessageFlags::empty()),
        guild_id: guild_id.map(Id::new),
        id: Id::new(id),
        #[allow(deprecated)]
        interaction: None,
        interaction_metadata: None,
        kind: MessageType::Regular,
        member: None,
        mention_channels: Vec::new(),
        mention_everyone: false,
        mention_roles: Vec::new(),
        mentions: Vec::new(),
        message_snapshots: Vec::new(),
        pinned: false,
        poll: None,
        reactions: Vec::new(),
        reference: None,
        referenced_message: None,
        role_subscription_data: None,
        sticker_items: Vec::new(),
        timestamp: Timestamp::from_secs(1).unwrap(),
        thread: None,
        tts: false,
        webhook_id: None,
    }
}

pub fn ready_event(user_id: u64, guild_ids: &[u64]) -> Ready {
    Ready {
        application: PartialApplication {
            flags: twilight_model::oauth::ApplicationFlags::empty(),
            id: Id::<ApplicationMarker>::new(1),
        },
        guilds: guild_ids
            .iter()
            .map(|id| UnavailableGuild { id: Id::new(*id), unavailable: true })
            .collect(),
        resume_gateway_url: String::new(),
        session_id: String::new(),
        shard: None,
        user: CurrentUser {
            accent_color: None,
            avatar: None,
            banner: None,
            bot: false,
            discriminator: 0,
            email: None,
            flags: None,
            id: Id::new(user_id),
            locale: None,
            mfa_enabled: false,
            name: "bot".to_owned(),
            premium_type: None,
            public_flags: None,
            verified: None,
        },
        version: 0,
    }
}
