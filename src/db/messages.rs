use rusqlite::{Connection, OptionalExtension, params};
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker},
};

use super::{Db, id_from_sql, id_to_sql};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotMessageKind {
    Status,
    RolePicker,
}

impl BotMessageKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::RolePicker => "role",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BotMessage {
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub digest: Option<u64>,
}

/// A failed read counts as a match, so the caller still does its locked check.
pub async fn stored_among(
    db: &Db,
    guild: Id<GuildMarker>,
    kind: BotMessageKind,
    ids: &[Id<MessageMarker>],
) -> bool {
    match db
        .call(move |c| get(c, guild, kind))
        .await
    {
        Ok(Some(record)) => ids.contains(&record.message_id),
        Ok(None) => false,
        Err(_) => true,
    }
}

pub fn get(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: BotMessageKind,
) -> rusqlite::Result<Option<BotMessage>> {
    conn.query_row(
        "SELECT channel_id, message_id, digest FROM bot_message WHERE guild_id = ?1 AND kind = ?2",
        params![id_to_sql(guild.get()), kind.as_str()],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        },
    )
    .optional()
    .map(|row| {
        row.and_then(|(channel, message, digest)| {
            Some(BotMessage {
                channel_id: Id::new_checked(id_from_sql(channel))?,
                message_id: Id::new_checked(id_from_sql(message))?,
                digest: digest.map(id_from_sql),
            })
        })
    })
}

pub fn set(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: BotMessageKind,
    message: BotMessage,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO bot_message (guild_id, kind, channel_id, message_id, digest)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (guild_id, kind) DO UPDATE SET channel_id = excluded.channel_id,
             message_id = excluded.message_id, digest = excluded.digest",
        params![
            id_to_sql(guild.get()),
            kind.as_str(),
            id_to_sql(message.channel_id.get()),
            id_to_sql(message.message_id.get()),
            message.digest.map(id_to_sql)
        ],
    )?;
    Ok(())
}

pub fn delete(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: BotMessageKind,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM bot_message WHERE guild_id = ?1 AND kind = ?2",
        params![id_to_sql(guild.get()), kind.as_str()],
    )?;
    Ok(())
}
