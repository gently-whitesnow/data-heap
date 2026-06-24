//! SQLite adapter implementing the [`Storage`](crate::domain::ports::Storage) port.
//!
//! `rusqlite` is blocking, so the async trait wraps every call in
//! `tokio::task::spawn_blocking`. The connection lives behind `Arc<Mutex<…>>`
//! so the blocking closure can own a `'static` handle without touching the
//! `tokio` runtime from inside it.

mod mapping;
mod migrations;
mod ops;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use rusqlite::Connection;

use crate::domain::error::{Error, Result};
use crate::domain::item::{Item, ItemId, NewItem};
use crate::domain::ports::Storage;
use crate::domain::source::{Source, Space};

/// SQLite busy timeout — how long a writer waits on a lock held by another
/// connection before failing with `SQLITE_BUSY`. Five seconds covers slow
/// fsyncs without hanging the request.
const BUSY_TIMEOUT_MS: u32 = 5_000;

pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStorage {
    /// Open (creating if needed) a database file and apply migrations.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        tokio::task::spawn_blocking(move || {
            let conn = Connection::open(path)?;
            Self::from_connection(conn)
        })
        .await?
    }

    /// In-memory database, for tests.
    pub async fn open_in_memory() -> Result<Self> {
        tokio::task::spawn_blocking(|| {
            let conn = Connection::open_in_memory()?;
            Self::from_connection(conn)
        })
        .await?
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_millis(u64::from(BUSY_TIMEOUT_MS)))?;
        let storage = SqliteStorage {
            conn: Arc::new(Mutex::new(conn)),
        };
        migrations::run(&storage.lock())?;
        Ok(storage)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A poisoned lock means a prior panic mid-query; recovering the guard is
        // safe here since each call runs a self-contained statement.
        self.conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    async fn run<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            f(&mut guard)
        })
        .await?
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn migrate(&self) -> Result<()> {
        self.run(|conn| migrations::run(conn)).await
    }

    async fn upsert_source(&self, source: Source) -> Result<()> {
        self.run(move |conn| ops::upsert_source(conn, &source)).await
    }

    async fn list_sources(&self) -> Result<Vec<Source>> {
        self.run(|conn| ops::list_sources(conn)).await
    }

    async fn insert_item(&self, item: NewItem) -> Result<ItemId> {
        let now = now_unix()?;
        self.run(move |conn| ops::insert_item(conn, &item, now))
            .await
    }

    async fn insert_items(&self, items: Vec<NewItem>) -> Result<Vec<ItemId>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let now = now_unix()?;
        self.run(move |conn| ops::insert_items(conn, &items, now))
            .await
    }

    async fn get_item(&self, id: ItemId) -> Result<Option<Item>> {
        self.run(move |conn| ops::get_item(conn, id)).await
    }

    async fn fetch_unprocessed(
        &self,
        agent_slug: &str,
        space: &Space,
        limit: u32,
    ) -> Result<Vec<Item>> {
        let agent_slug = agent_slug.to_owned();
        let space = space.as_str().to_owned();
        self.run(move |conn| ops::fetch_unprocessed(conn, &agent_slug, &space, limit))
            .await
    }

    async fn mark_processed(&self, agent_slug: &str, item_id: ItemId) -> Result<()> {
        let agent_slug = agent_slug.to_owned();
        let now = now_unix()?;
        self.run(move |conn| ops::mark_processed(conn, &agent_slug, item_id, now))
            .await
    }

    async fn delete_item(&self, item_id: ItemId) -> Result<()> {
        let now = now_unix()?;
        self.run(move |conn| ops::delete_item(conn, item_id, now))
            .await
    }
}

fn now_unix() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(Error::from)
}

#[cfg(test)]
mod tests;
