use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub etag: Option<String>,
    pub fetched_at: i64,
    pub body: Vec<u8>,
}

pub fn get(conn: &mut Connection, name: &str) -> rusqlite::Result<Option<Stored>> {
    conn.query_row(
        "SELECT etag, fetched_at, body FROM catalog WHERE name = ?1",
        params![name],
        |row| Ok(Stored { etag: row.get(0)?, fetched_at: row.get(1)?, body: row.get(2)? }),
    )
    .optional()
}

pub fn put(conn: &mut Connection, name: &str, stored: &Stored) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO catalog (name, etag, fetched_at, body) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (name) DO UPDATE SET etag = excluded.etag, fetched_at = excluded.fetched_at,
             body = excluded.body",
        params![name, stored.etag, stored.fetched_at, stored.body],
    )?;
    Ok(())
}

pub fn touch(conn: &mut Connection, name: &str, fetched_at: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE catalog SET fetched_at = ?2 WHERE name = ?1",
        params![name, fetched_at],
    )?;
    Ok(())
}
