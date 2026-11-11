mod schema;

pub mod ai;
pub mod catalog;
pub mod guild;
pub mod messages;
pub mod notify;
pub mod quarantine;

use std::{path::Path, sync::mpsc, thread};

use anyhow::Context as _;
use rusqlite::Connection;
use tokio::sync::oneshot;

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

#[derive(Clone)]
pub struct Db {
    tx: mpsc::Sender<Job>,
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("create database directory {}", dir.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("open database {}", path.display()))?;
        // A read-only file opens fine, but every write would fail while health looks good.
        anyhow::ensure!(
            !conn.is_readonly(rusqlite::MAIN_DB)?,
            "database {} is read-only; it must be writable by the bot's user",
            path.display()
        );
        Self::start(conn)
    }

    pub fn open_in_memory() -> anyhow::Result<Self> {
        Self::start(Connection::open_in_memory()?)
    }

    fn start(mut conn: Connection) -> anyhow::Result<Self> {
        configure(&conn)?;
        // Catches a stale -wal or -shm file owned by another user.
        conn.execute_batch("BEGIN IMMEDIATE; COMMIT;")
            .context("database is not writable by the bot's user")?;
        schema::migrate(&mut conn)?;
        let (tx, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("sqlite".into())
            .spawn(move || {
                // A panicking job fails only its caller; its transaction rolls back on unwind.
                while let Ok(job) = rx.recv() {
                    let _ =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&mut conn)));
                }
            })
            .context("spawn sqlite worker")?;
        Ok(Self { tx })
    }

    pub async fn call<R, F>(&self, f: F) -> anyhow::Result<R>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> rusqlite::Result<R> + Send + 'static,
    {
        let (done, wait) = oneshot::channel();
        self.tx
            .send(Box::new(move |conn| {
                let _ = done.send(f(conn));
            }))
            .map_err(|_| anyhow::anyhow!("sqlite worker stopped"))?;
        Ok(wait
            .await
            .context("sqlite worker dropped the job")??)
    }
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
}

pub fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}

pub(crate) fn id_to_sql(id: u64) -> i64 {
    id as i64
}

pub(crate) fn id_from_sql(id: i64) -> u64 {
    id as u64
}
