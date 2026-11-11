use rusqlite::{Connection, OptionalExtension, Row, params};
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use super::{id_from_sql, id_to_sql};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Spam,
    Scam,
    Manual,
}

impl Reason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spam => "spam",
            Self::Scam => "scam",
            Self::Manual => "manual",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "scam" => Self::Scam,
            "manual" => Self::Manual,
            _ => Self::Spam,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Role swap not confirmed yet. Startup finishes it instead of releasing.
    Applying,
    Active,
    Releasing,
    /// The Quarantine role was replaced and must be given again. Startup re-applies it.
    Reapply,
}

impl State {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Applying => "applying",
            Self::Active => "active",
            Self::Releasing => "releasing",
            Self::Reapply => "reapply",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "applying" => Self::Applying,
            "releasing" => Self::Releasing,
            "reapply" => Self::Reapply,
            _ => Self::Active,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub guild_id: Id<GuildMarker>,
    pub user_id: Id<UserMarker>,
    pub token: String,
    pub roles: Vec<Id<RoleMarker>>,
    pub state: State,
    pub reason: Reason,
    pub evidence: String,
    pub case_message: Option<(Id<ChannelMarker>, Id<MessageMarker>)>,
    pub created_at: i64,
    /// Only staff may end a kicked case.
    pub kicked: bool,
}

const COLUMNS: &str = "guild_id, user_id, token, roles, state, reason, evidence, case_channel_id, case_message_id, created_at, kicked";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Record> {
    let roles: String = row.get(3)?;
    let roles = roles
        .split(',')
        .filter_map(|raw| raw.parse::<u64>().ok())
        .filter_map(Id::new_checked)
        .collect();
    let state: String = row.get(4)?;
    let reason: String = row.get(5)?;
    let case_channel: Option<i64> = row.get(7)?;
    let case_message: Option<i64> = row.get(8)?;
    Ok(Record {
        guild_id: Id::new(id_from_sql(row.get(0)?)),
        user_id: Id::new(id_from_sql(row.get(1)?)),
        token: row.get(2)?,
        roles,
        state: State::parse(&state),
        reason: Reason::parse(&reason),
        evidence: row.get(6)?,
        case_message: case_channel
            .zip(case_message)
            .and_then(|(c, m)| {
                Some((
                    Id::new_checked(id_from_sql(c))?,
                    Id::new_checked(id_from_sql(m))?,
                ))
            }),
        created_at: row.get(9)?,
        kicked: row.get::<_, i64>(10)? != 0,
    })
}

fn encode_roles(roles: &[Id<RoleMarker>]) -> String {
    let mut out = String::with_capacity(roles.len() * 20);
    for (index, role) in roles.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&role.get().to_string());
    }
    out
}

pub fn get(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
) -> rusqlite::Result<Option<Record>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM quarantine WHERE guild_id = ?1 AND user_id = ?2"),
        params![id_to_sql(guild.get()), id_to_sql(user.get())],
        from_row,
    )
    .optional()
}

pub fn list(conn: &mut Connection) -> rusqlite::Result<Vec<Record>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM quarantine"))?;
    stmt.query_map([], from_row)?.collect()
}

/// Also calls off a running release, so a rejoin is quarantined again.
pub fn set_kicked(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "UPDATE quarantine SET kicked = 1,
             state = CASE state WHEN 'releasing' THEN 'active' ELSE state END
         WHERE guild_id = ?1 AND user_id = ?2",
        params![id_to_sql(guild.get()), id_to_sql(user.get())],
    )?;
    Ok(changed > 0)
}

pub fn insert_new(conn: &mut Connection, record: &Record) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        &format!(
            "INSERT INTO quarantine ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (guild_id, user_id) DO NOTHING"
        ),
        params![
            id_to_sql(record.guild_id.get()),
            id_to_sql(record.user_id.get()),
            record.token,
            encode_roles(&record.roles),
            record.state.as_str(),
            record.reason.as_str(),
            record.evidence,
            record
                .case_message
                .map(|(c, _)| id_to_sql(c.get())),
            record
                .case_message
                .map(|(_, m)| id_to_sql(m.get())),
            record.created_at,
            record.kicked,
        ],
    )?;
    Ok(changed == 1)
}

/// Other states are left alone: a swap or release in progress finishes on its own.
pub fn mark_active_for_reapply(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE quarantine SET state = ?2 WHERE guild_id = ?1 AND state = ?3",
        params![id_to_sql(guild.get()), State::Reapply.as_str(), State::Active.as_str()],
    )
}

pub fn set_reason(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    reason: Reason,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE quarantine SET reason = ?3 WHERE guild_id = ?1 AND user_id = ?2",
        params![id_to_sql(guild.get()), id_to_sql(user.get()), reason.as_str()],
    )?;
    Ok(())
}

pub fn set_roles(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    roles: &[Id<RoleMarker>],
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE quarantine SET roles = ?3 WHERE guild_id = ?1 AND user_id = ?2",
        params![id_to_sql(guild.get()), id_to_sql(user.get()), encode_roles(roles)],
    )?;
    Ok(())
}

pub fn set_state(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    state: State,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE quarantine SET state = ?3 WHERE guild_id = ?1 AND user_id = ?2",
        params![id_to_sql(guild.get()), id_to_sql(user.get()), state.as_str()],
    )?;
    Ok(())
}

pub fn set_case_message(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
    channel: Id<ChannelMarker>,
    message: Id<MessageMarker>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE quarantine SET case_channel_id = ?3, case_message_id = ?4
         WHERE guild_id = ?1 AND user_id = ?2",
        params![
            id_to_sql(guild.get()),
            id_to_sql(user.get()),
            id_to_sql(channel.get()),
            id_to_sql(message.get())
        ],
    )?;
    Ok(())
}

pub fn delete(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM quarantine WHERE guild_id = ?1 AND user_id = ?2",
        params![id_to_sql(guild.get()), id_to_sql(user.get())],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn record(token: &str) -> Record {
        Record {
            guild_id: Id::new(1),
            user_id: Id::new(2),
            token: token.into(),
            roles: vec![Id::new(30), Id::new(40)],
            state: State::Active,
            reason: Reason::Scam,
            evidence: "casino bonus".into(),
            case_message: None,
            created_at: 7,
            kicked: false,
        }
    }

    #[tokio::test]
    async fn kicking_keeps_the_reason_and_calls_off_a_release() {
        let db = Db::open_in_memory().unwrap();
        let releasing = Record { state: State::Releasing, ..record("1") };
        db.call(move |c| insert_new(c, &releasing))
            .await
            .unwrap();
        assert!(
            db.call(|c| set_kicked(c, Id::new(1), Id::new(2)))
                .await
                .unwrap()
        );
        let stored = db
            .call(|c| get(c, Id::new(1), Id::new(2)))
            .await
            .unwrap()
            .unwrap();
        assert!(stored.kicked);
        assert_eq!(stored.state, State::Active);
        assert_eq!(stored.reason, Reason::Scam);
        assert!(
            !db.call(|c| set_kicked(c, Id::new(1), Id::new(9)))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn only_active_cases_are_marked_for_reapply() {
        let db = Db::open_in_memory().unwrap();
        let releasing = Record { user_id: Id::new(3), state: State::Releasing, ..record("1") };
        let applying = Record { user_id: Id::new(4), state: State::Applying, ..record("3") };
        let other_guild = Record { guild_id: Id::new(9), ..record("2") };
        for r in [record("0"), releasing, applying, other_guild] {
            db.call(move |c| insert_new(c, &r))
                .await
                .unwrap();
        }
        let marked = db
            .call(|c| mark_active_for_reapply(c, Id::new(1)))
            .await
            .unwrap();
        assert_eq!(marked, 1);
        let mut states: Vec<(u64, u64, State)> = db
            .call(list)
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.guild_id.get(), r.user_id.get(), r.state))
            .collect();
        states.sort_by_key(|s| (s.0, s.1));
        assert_eq!(
            states,
            [
                (1, 2, State::Reapply),
                (1, 3, State::Releasing),
                (1, 4, State::Applying),
                (9, 2, State::Active)
            ]
        );
        db.call(|c| set_reason(c, Id::new(1), Id::new(2), Reason::Manual))
            .await
            .unwrap();
        db.call(|c| set_roles(c, Id::new(1), Id::new(2), &[Id::new(77)]))
            .await
            .unwrap();
        let stored = db
            .call(|c| get(c, Id::new(1), Id::new(2)))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.reason, Reason::Manual);
        assert_eq!(stored.roles, [Id::new(77)]);
    }

    #[tokio::test]
    async fn insert_is_first_writer_wins_and_keeps_leading_zero_tokens() {
        let db = Db::open_in_memory().unwrap();
        assert!(
            db.call(|c| insert_new(c, &record("000123")))
                .await
                .unwrap()
        );
        assert!(
            !db.call(|c| insert_new(c, &record("999999")))
                .await
                .unwrap()
        );
        let stored = db
            .call(|c| get(c, Id::new(1), Id::new(2)))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.token, "000123");
        assert_eq!(stored.roles, vec![Id::new(30), Id::new(40)]);
        assert_eq!(stored.reason, Reason::Scam);

        db.call(|c| set_state(c, Id::new(1), Id::new(2), State::Releasing))
            .await
            .unwrap();
        db.call(|c| set_case_message(c, Id::new(1), Id::new(2), Id::new(5), Id::new(6)))
            .await
            .unwrap();
        let stored = db
            .call(|c| get(c, Id::new(1), Id::new(2)))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.state, State::Releasing);
        assert_eq!(
            stored.case_message,
            Some((Id::new(5), Id::new(6)))
        );

        db.call(|c| delete(c, Id::new(1), Id::new(2)))
            .await
            .unwrap();
        assert!(
            db.call(|c| get(c, Id::new(1), Id::new(2)))
                .await
                .unwrap()
                .is_none()
        );
    }
}
