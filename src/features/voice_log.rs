use std::{collections::HashMap, sync::Arc};

use twilight_model::{
    channel::message::MessageFlags,
    id::{
        Id,
        marker::{ChannelMarker, GuildMarker, UserMarker},
    },
    user::User,
    voice::VoiceState,
};

use crate::{
    app::{self, App},
    discord::ui::{self, Card, color},
    guild::ChannelKind,
};

/// Past this a newcomer isn't tracked, or each of their mutes would log as a join.
const MAX_TRACKED: usize = 4096;

#[derive(Default)]
pub struct Voices {
    channels: HashMap<(Id<GuildMarker>, Id<UserMarker>), Id<ChannelMarker>>,
}

impl Voices {
    /// Returns the previous channel. An untracked member at the cap gets the current one, so
    /// nothing is logged.
    pub fn update(
        &mut self,
        guild: Id<GuildMarker>,
        user: Id<UserMarker>,
        channel: Option<Id<ChannelMarker>>,
    ) -> Option<Id<ChannelMarker>> {
        let Some(channel) = channel else {
            return self.channels.remove(&(guild, user));
        };
        if self.channels.len() < MAX_TRACKED
            || self
                .channels
                .contains_key(&(guild, user))
        {
            self.channels
                .insert((guild, user), channel)
        } else {
            Some(channel)
        }
    }

    pub fn remove(
        &mut self,
        guild: Id<GuildMarker>,
        user: Id<UserMarker>,
    ) -> Option<Id<ChannelMarker>> {
        self.channels.remove(&(guild, user))
    }

    /// Drops guilds left while the bot was offline.
    pub fn keep_guilds(&mut self, guilds: &[Id<GuildMarker>]) {
        self.channels
            .retain(|(g, _), _| guilds.contains(g));
    }

    pub fn reset(&mut self, guild: Id<GuildMarker>, states: &[VoiceState]) {
        self.channels
            .retain(|(g, _), _| *g != guild);
        for state in states {
            if let Some(channel) = state.channel_id {
                self.update(guild, state.user_id, Some(channel));
            }
        }
        if self.channels.capacity() > 4 * self.channels.len().max(64) {
            self.channels.shrink_to_fit();
        }
    }

    pub fn tracked(&self) -> usize {
        self.channels.len()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Joined(Id<ChannelMarker>),
    Left(Id<ChannelMarker>),
    Moved(Id<ChannelMarker>, Id<ChannelMarker>),
}

pub fn classify(
    previous: Option<Id<ChannelMarker>>,
    current: Option<Id<ChannelMarker>>,
) -> Option<Change> {
    match (previous, current) {
        (None, Some(now)) => Some(Change::Joined(now)),
        (Some(before), None) => Some(Change::Left(before)),
        (Some(before), Some(now)) if before != now => Some(Change::Moved(before, now)),
        _ => None,
    }
}

pub async fn on_update(app: &Arc<App>, state: &VoiceState, previous: Option<Id<ChannelMarker>>) {
    let Some(guild) = state.guild_id else {
        return;
    };
    let Some(change) = classify(previous, state.channel_id) else {
        return;
    };
    match state.member.as_ref() {
        Some(member) => post(
            app,
            guild,
            Some((&member.user, member.nick.as_deref())),
            state.user_id,
            change,
        ),
        // Discord may leave out the member after they left; name them by mention.
        None if state.user_id != app.bot_id => post(app, guild, None, state.user_id, change),
        None => {}
    }
}

/// Discord's disconnect can come after MemberRemove, which already untracked them, so log here.
pub fn on_member_leave(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    user: &User,
    channel: Id<ChannelMarker>,
) {
    post(
        app,
        guild,
        Some((user, None)),
        user.id,
        Change::Left(channel),
    );
}

fn post(
    app: &Arc<App>,
    guild: Id<GuildMarker>,
    member: Option<(&User, Option<&str>)>,
    user_id: Id<UserMarker>,
    change: Change,
) {
    if member.is_some_and(|(user, _)| user.bot) {
        return;
    }
    let Some(log) = app
        .guild(guild)
        .channel(ChannelKind::VoiceLog)
    else {
        return;
    };
    let name = match member {
        Some((user, nick)) => format!(
            "**{}**",
            ui::escape_markdown(nick.unwrap_or_else(|| app::display_name(user)))
        ),
        None => format!("<@{user_id}>"),
    };
    let line = match change {
        Change::Joined(ch) => format!("{name} เข้าห้อง <#{ch}>"),
        Change::Left(ch) => format!("{name} ออกจากห้อง <#{ch}>"),
        Change::Moved(from, to) => format!("{name} ย้ายจาก <#{from}> ไป <#{to}>"),
    };
    let accent = match change {
        Change::Joined(_) => color::SUCCESS,
        Change::Left(_) => color::DANGER,
        Change::Moved(..) => color::INFO,
    };
    let now = jiff::Timestamp::now().as_millisecond();
    let avatar = member.map(|(user, _)| app::avatar_url(user));
    let card = Card::new(accent)
        .section(
            format!(
                "{line}\n-# <@{user_id}> · {} ({})",
                ui::ts(now, 'T'),
                ui::ts(now, 'R')
            ),
            avatar.as_deref(),
        )
        .build();
    let app2 = Arc::clone(app);
    app.spawn_outbound("voice_log", async move {
        if let Err(error) = app2
            .http
            .create_message(log)
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(&[card])
            .await
        {
            tracing::warn!(%error, "failed to send voice log");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_tracking_is_capped_and_reset_per_guild() {
        let mut voices = Voices::default();
        let (a, b) = (Id::new(1), Id::new(2));
        for user in 1..=MAX_TRACKED as u64 {
            assert_eq!(
                voices.update(a, Id::new(user), Some(Id::new(9))),
                None
            );
        }
        // Past the cap a newcomer isn't tracked, but tracked members still move and leave.
        assert_eq!(
            voices.update(b, Id::new(1), Some(Id::new(9))),
            Some(Id::new(9))
        );
        assert_eq!(
            voices.update(b, Id::new(1), Some(Id::new(7))),
            Some(Id::new(7))
        );
        assert_eq!(voices.update(b, Id::new(1), None), None);
        assert_eq!(
            voices.update(a, Id::new(1), Some(Id::new(8))),
            Some(Id::new(9))
        );
        assert_eq!(
            voices.update(a, Id::new(1), None),
            Some(Id::new(8))
        );
        assert_eq!(voices.tracked(), MAX_TRACKED - 1);
        // Whichever of MemberRemove and the disconnect comes first returns the channel.
        assert_eq!(voices.remove(a, Id::new(2)), Some(Id::new(9)));
        assert_eq!(voices.update(a, Id::new(2), None), None);
        assert_eq!(
            voices.update(a, Id::new(3), None),
            Some(Id::new(9))
        );
        assert_eq!(voices.remove(a, Id::new(3)), None);
        voices.update(a, Id::new(3), Some(Id::new(9)));
        assert_eq!(voices.tracked(), MAX_TRACKED - 2);
        voices.update(b, Id::new(1), Some(Id::new(7)));
        voices.keep_guilds(&[b]);
        assert_eq!(voices.tracked(), 1);
        voices.reset(b, &[]);
        assert_eq!(voices.tracked(), 0);
    }

    #[test]
    fn mute_and_deafen_updates_are_ignored() {
        let a = Some(Id::new(1));
        let b = Some(Id::new(2));
        assert_eq!(classify(a, a), None);
        assert_eq!(classify(None, None), None);
        assert_eq!(
            classify(None, a),
            Some(Change::Joined(Id::new(1)))
        );
        assert_eq!(classify(a, None), Some(Change::Left(Id::new(1))));
        assert_eq!(
            classify(a, b),
            Some(Change::Moved(Id::new(1), Id::new(2)))
        );
    }
}
