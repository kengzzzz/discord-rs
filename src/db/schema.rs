use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE guild_channel (
    guild_id   INTEGER NOT NULL,
    kind       TEXT    NOT NULL,
    channel_id INTEGER NOT NULL,
    PRIMARY KEY (guild_id, kind)
) STRICT;

CREATE TABLE guild_role (
    guild_id        INTEGER NOT NULL,
    kind            TEXT    NOT NULL,
    role_id         INTEGER NOT NULL,
    self_assignable INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (guild_id, kind)
) STRICT;

CREATE TABLE guild_settings (
    guild_id    INTEGER PRIMARY KEY,
    scam_detect INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE bot_message (
    guild_id   INTEGER NOT NULL,
    kind       TEXT    NOT NULL,
    channel_id INTEGER NOT NULL,
    message_id INTEGER NOT NULL,
    digest     INTEGER,
    PRIMARY KEY (guild_id, kind)
) STRICT;

CREATE TABLE quarantine (
    guild_id        INTEGER NOT NULL,
    user_id         INTEGER NOT NULL,
    token           TEXT    NOT NULL,
    roles           TEXT    NOT NULL,
    state           TEXT    NOT NULL,
    reason          TEXT    NOT NULL,
    evidence        TEXT    NOT NULL DEFAULT '',
    case_channel_id INTEGER,
    case_message_id INTEGER,
    created_at      INTEGER NOT NULL,
    PRIMARY KEY (guild_id, user_id)
) STRICT;

CREATE TABLE ai_prompt (
    user_id    INTEGER PRIMARY KEY,
    prompt     TEXT    NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE ai_turn (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL,
    scope      TEXT    NOT NULL,
    role       TEXT    NOT NULL,
    text       TEXT    NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX ai_turn_by_user ON ai_turn (user_id, scope, id);

CREATE TABLE notify_log (
    guild_id   INTEGER NOT NULL,
    kind       TEXT    NOT NULL,
    period     TEXT    NOT NULL,
    state      TEXT    NOT NULL,
    message_id INTEGER,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (guild_id, kind, period)
) STRICT;

CREATE TABLE catalog (
    name       TEXT PRIMARY KEY,
    etag       TEXT,
    fetched_at INTEGER NOT NULL,
    body       BLOB NOT NULL
) STRICT;
"#,
    r#"
-- A kicked member's case stays open for staff only, whatever its reason.
ALTER TABLE quarantine ADD COLUMN kicked INTEGER NOT NULL DEFAULT 0;
"#,
];

pub fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (index, sql) in MIGRATIONS
        .iter()
        .enumerate()
        .skip(current.max(0) as usize)
    {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}
