//! SQLite adapter implementing the [`Storage`](crate::domain::ports::Storage) port.
//!
//! Built on `sqlx`: a native async pool, typed row decoding, and built-in
//! `sqlx::migrate!()`. No `spawn_blocking` wrappers — the pool drives I/O on
//! tokio directly.

mod mapping;
mod ops;

use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};

use crate::domain::error::{Error, Result};
use crate::domain::item::{Item, ItemId, NewItem};
use crate::domain::ports::Storage;
use crate::domain::source::{Source, Space};

/// SQLite busy timeout — how long a writer waits on a lock held by another
/// connection before failing with `SQLITE_BUSY`. Five seconds covers slow
/// fsyncs without hanging the request.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("src/adapters/sqlite/migrations");

pub struct SqliteStorage {
    pool: Pool<Sqlite>,
}

impl SqliteStorage {
    /// Open (creating if needed) a database file and apply migrations.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(BUSY_TIMEOUT);
        let pool = SqlitePoolOptions::new().connect_with(opts).await?;
        Self::from_pool(pool).await
    }

    /// In-memory database, for tests. The pool is pinned to one connection
    /// because `sqlite::memory:` is per-connection — multiple pool connections
    /// would see independent empty databases.
    pub async fn open_in_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?
            .foreign_keys(true)
            .busy_timeout(BUSY_TIMEOUT);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        Self::from_pool(pool).await
    }

    async fn from_pool(pool: Pool<Sqlite>) -> Result<Self> {
        MIGRATOR.run(&pool).await?;
        Ok(SqliteStorage { pool })
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn migrate(&self) -> Result<()> {
        MIGRATOR.run(&self.pool).await.map_err(Error::from)
    }

    async fn upsert_source(&self, source: Source) -> Result<()> {
        ops::upsert_source(&self.pool, &source).await
    }

    async fn list_sources(&self) -> Result<Vec<Source>> {
        ops::list_sources(&self.pool).await
    }

    async fn insert_item(&self, item: NewItem) -> Result<ItemId> {
        let now = now_unix()?;
        ops::insert_item(&self.pool, &item, now).await
    }

    async fn insert_items(&self, items: Vec<NewItem>) -> Result<Vec<ItemId>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let now = now_unix()?;
        ops::insert_items(&self.pool, &items, now).await
    }

    async fn get_item(&self, id: ItemId) -> Result<Option<Item>> {
        ops::get_item(&self.pool, id).await
    }

    async fn fetch_unprocessed(
        &self,
        agent_slug: &str,
        space: &Space,
        limit: u32,
    ) -> Result<Vec<Item>> {
        ops::fetch_unprocessed(&self.pool, agent_slug, space.as_str(), limit).await
    }

    async fn mark_processed(&self, agent_slug: &str, item_id: ItemId) -> Result<()> {
        let now = now_unix()?;
        ops::mark_processed(&self.pool, agent_slug, item_id, now).await
    }

    async fn delete_item(&self, item_id: ItemId) -> Result<()> {
        let now = now_unix()?;
        ops::delete_item(&self.pool, item_id, now).await
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
