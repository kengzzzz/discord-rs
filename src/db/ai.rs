use rusqlite::{Connection, OptionalExtension, params};
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, UserMarker},
};

use super::id_to_sql;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Private,
    /// Keyed by channel, so staff-channel chat never becomes context for a public answer.
    Public(Id<ChannelMarker>),
}

impl Scope {
    fn key(self) -> String {
        match self {
            Self::Private => "private".to_owned(),
            Self::Public(channel) => format!("public:{channel}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub from_model: bool,
    pub text: String,
}

pub fn get_prompt(conn: &mut Connection, user: Id<UserMarker>) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT prompt FROM ai_prompt WHERE user_id = ?1",
        params![id_to_sql(user.get())],
        |row| row.get(0),
    )
    .optional()
}

pub fn set_prompt(
    conn: &mut Connection,
    user: Id<UserMarker>,
    prompt: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ai_prompt (user_id, prompt, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (user_id) DO UPDATE SET prompt = excluded.prompt, updated_at = excluded.updated_at",
        params![id_to_sql(user.get()), prompt, now],
    )?;
    Ok(())
}

pub fn clear_prompt(conn: &mut Connection, user: Id<UserMarker>) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "DELETE FROM ai_prompt WHERE user_id = ?1",
        params![id_to_sql(user.get())],
    )? > 0)
}

pub fn load_turns(
    conn: &mut Connection,
    user: Id<UserMarker>,
    scope: Scope,
    since: i64,
    limit: usize,
) -> rusqlite::Result<Vec<Turn>> {
    let mut stmt = conn.prepare_cached(
        "SELECT role, text FROM ai_turn
         WHERE user_id = ?1 AND scope = ?2 AND created_at >= ?3
         ORDER BY id DESC LIMIT ?4",
    )?;
    let mut turns: Vec<Turn> = stmt
        .query_map(
            params![id_to_sql(user.get()), scope.key(), since, limit as i64],
            |row| Ok(Turn { from_model: row.get::<_, String>(0)? == "model", text: row.get(1)? }),
        )?
        .collect::<rusqlite::Result<_>>()?;
    turns.reverse();
    Ok(turns)
}

pub fn append_exchange(
    conn: &mut Connection,
    user: Id<UserMarker>,
    scope: Scope,
    question: &str,
    answer: &str,
    now: i64,
    keep: usize,
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        let mut insert = tx.prepare_cached(
            "INSERT INTO ai_turn (user_id, scope, role, text, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        insert.execute(params![
            id_to_sql(user.get()),
            scope.key(),
            "user",
            question,
            now
        ])?;
        insert.execute(params![
            id_to_sql(user.get()),
            scope.key(),
            "model",
            answer,
            now
        ])?;
        tx.execute(
            "DELETE FROM ai_turn WHERE user_id = ?1 AND scope = ?2 AND id NOT IN (
                 SELECT id FROM ai_turn WHERE user_id = ?1 AND scope = ?2 ORDER BY id DESC LIMIT ?3)",
            params![id_to_sql(user.get()), scope.key(), keep as i64],
        )?;
    }
    tx.commit()
}

pub fn clear_turns(conn: &mut Connection, user: Id<UserMarker>) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM ai_turn WHERE user_id = ?1",
        params![id_to_sql(user.get())],
    )
}

pub fn prune_turns(conn: &mut Connection, older_than: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM ai_turn WHERE created_at < ?1",
        params![older_than],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[tokio::test]
    async fn history_is_scoped_trimmed_and_expires() {
        let db = Db::open_in_memory().unwrap();
        let user = Id::new(9);
        for i in 0..5 {
            db.call(move |c| {
                append_exchange(
                    c,
                    user,
                    Scope::Public(Id::new(5)),
                    &format!("q{i}"),
                    &format!("a{i}"),
                    100 + i,
                    4,
                )
            })
            .await
            .unwrap();
        }
        db.call(move |c| append_exchange(c, user, Scope::Private, "secret", "ok", 200, 4))
            .await
            .unwrap();

        let public = db
            .call(move |c| load_turns(c, user, Scope::Public(Id::new(5)), 0, 10))
            .await
            .unwrap();
        let texts: Vec<&str> = public
            .iter()
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(texts, ["q3", "a3", "q4", "a4"]);
        assert!(!public[0].from_model && public[1].from_model);

        let recent = db
            .call(move |c| load_turns(c, user, Scope::Public(Id::new(5)), 104, 10))
            .await
            .unwrap();
        assert_eq!(recent.len(), 2);

        assert_eq!(
            db.call(move |c| prune_turns(c, 104))
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            db.call(move |c| clear_turns(c, user))
                .await
                .unwrap(),
            4
        );
    }

    #[tokio::test]
    async fn prompts_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let user = Id::new(3);
        db.call(move |c| set_prompt(c, user, "speak like Arona", 1))
            .await
            .unwrap();
        assert_eq!(
            db.call(move |c| get_prompt(c, user))
                .await
                .unwrap()
                .as_deref(),
            Some("speak like Arona")
        );
        assert!(
            db.call(move |c| clear_prompt(c, user))
                .await
                .unwrap()
        );
        assert!(
            db.call(move |c| get_prompt(c, user))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn public_history_stays_in_its_channel() {
        let db = Db::open_in_memory().unwrap();
        let user = Id::new(9);
        db.call(move |c| {
            append_exchange(
                c,
                user,
                Scope::Public(Id::new(1)),
                "staff only",
                "ok",
                100,
                4,
            )
        })
        .await
        .unwrap();
        let other = db
            .call(move |c| load_turns(c, user, Scope::Public(Id::new(2)), 0, 10))
            .await
            .unwrap();
        assert!(other.is_empty());
    }
}
