use rusqlite::{Connection, OptionalExtension, params};
use twilight_model::id::{
    Id,
    marker::{GuildMarker, MessageMarker},
};

use super::{id_from_sql, id_to_sql};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Sent(Option<Id<MessageMarker>>),
}

pub fn get(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: &str,
    period: &str,
) -> rusqlite::Result<Option<Status>> {
    conn.query_row(
        "SELECT state, message_id FROM notify_log WHERE guild_id = ?1 AND kind = ?2 AND period = ?3",
        params![id_to_sql(guild.get()), kind, period],
        |row| {
            let state: String = row.get(0)?;
            let message: Option<i64> = row.get(1)?;
            Ok(if state == "sent" {
                Status::Sent(message.and_then(|m| Id::new_checked(id_from_sql(m))))
            } else {
                Status::Pending
            })
        },
    )
    .optional()
}

pub fn mark_pending(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: &str,
    period: &str,
    now: i64,
) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "INSERT INTO notify_log (guild_id, kind, period, state, updated_at) VALUES (?1, ?2, ?3, 'pending', ?4)
         ON CONFLICT (guild_id, kind, period) DO NOTHING",
        params![id_to_sql(guild.get()), kind, period, now],
    )?;
    Ok(changed == 1)
}

pub fn mark_sent(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: &str,
    period: &str,
    message: Option<Id<MessageMarker>>,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO notify_log (guild_id, kind, period, state, message_id, updated_at)
         VALUES (?1, ?2, ?3, 'sent', ?4, ?5)
         ON CONFLICT (guild_id, kind, period) DO UPDATE
         SET state = 'sent', message_id = excluded.message_id, updated_at = excluded.updated_at",
        params![id_to_sql(guild.get()), kind, period, message.map(|m| id_to_sql(m.get())), now],
    )?;
    Ok(())
}

pub fn forget(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: &str,
    period: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM notify_log WHERE guild_id = ?1 AND kind = ?2 AND period = ?3",
        params![id_to_sql(guild.get()), kind, period],
    )?;
    Ok(())
}

pub fn prune(conn: &mut Connection, older_than: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM notify_log WHERE updated_at < ?1",
        params![older_than],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[tokio::test]
    async fn pending_claim_is_exclusive_until_forgotten() {
        let db = Db::open_in_memory().unwrap();
        let g = Id::new(1);
        assert!(
            db.call(move |c| mark_pending(c, g, "baro", "w1", 1))
                .await
                .unwrap()
        );
        assert!(
            !db.call(move |c| mark_pending(c, g, "baro", "w1", 2))
                .await
                .unwrap()
        );
        assert_eq!(
            db.call(move |c| get(c, g, "baro", "w1"))
                .await
                .unwrap(),
            Some(Status::Pending)
        );
        db.call(move |c| mark_sent(c, g, "baro", "w1", Some(Id::new(55)), 3))
            .await
            .unwrap();
        assert_eq!(
            db.call(move |c| get(c, g, "baro", "w1"))
                .await
                .unwrap(),
            Some(Status::Sent(Some(Id::new(55))))
        );
        db.call(move |c| forget(c, g, "baro", "w1"))
            .await
            .unwrap();
        assert!(
            db.call(move |c| mark_pending(c, g, "baro", "w1", 4))
                .await
                .unwrap()
        );
    }
}
