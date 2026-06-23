//! SQLite adapter implementing the [`Storage`](crate::domain::ports::Storage) port.

mod mapping;
mod migrations;

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

use crate::domain::error::{Error, Result};
use crate::domain::item::{Item, ItemId, NewItem};
use crate::domain::ports::Storage;
use crate::domain::source::{Source, Space};

/// Single-connection SQLite storage. The connection is guarded by a `Mutex`
/// because `rusqlite::Connection` is `!Sync`; a connection pool can replace this
/// later without touching the port.
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    /// Open (creating if needed) a database file and apply migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).map_err(map_sqlite)?;
        Self::from_connection(conn)
    }

    /// In-memory database, for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(map_sqlite)?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(map_sqlite)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(map_sqlite)?;
        let storage = SqliteStorage {
            conn: Mutex::new(conn),
        };
        storage.migrate()?;
        Ok(storage)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A poisoned lock means a prior panic mid-query; recovering the guard is
        // safe here since each call runs a self-contained statement.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Storage for SqliteStorage {
    fn migrate(&self) -> Result<()> {
        migrations::run(&self.lock())
    }

    fn upsert_source(&self, source: &Source) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO sources (slug, space)
             VALUES (?1, ?2)
             ON CONFLICT(slug) DO UPDATE SET space = excluded.space",
            rusqlite::params![source.slug, source.space.as_str()],
        )
        .map_err(map_sqlite)?;
        Ok(())
    }

    fn list_sources(&self) -> Result<Vec<Source>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT slug, space FROM sources ORDER BY slug")
            .map_err(map_sqlite)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Source {
                    slug: row.get(0)?,
                    space: Space::new(row.get::<_, String>(1)?),
                })
            })
            .map_err(map_sqlite)?;
        let mut sources = Vec::new();
        for row in rows {
            sources.push(row.map_err(map_sqlite)?);
        }
        Ok(sources)
    }

    fn insert_item(&self, item: &NewItem) -> Result<ItemId> {
        let extra = mapping::extra_json(&item.telegram)?;
        let conn = self.lock();
        // Dedup on the Telegram message address: a repeated polling update is a
        // no-op that returns the id of the already-stored item.
        let inserted = conn
            .execute(
                "INSERT INTO items
                    (source, space, kind, text, chat_id, message_id, telegram_extra, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(chat_id, message_id) DO NOTHING",
                rusqlite::params![
                    item.source,
                    item.space.as_str(),
                    item.kind.as_str(),
                    item.text,
                    item.telegram.chat_id,
                    item.telegram.message_id,
                    extra,
                    now_unix(),
                ],
            )
            .map_err(map_sqlite)?;
        if inserted > 0 {
            return Ok(ItemId(conn.last_insert_rowid()));
        }
        conn.query_row(
            "SELECT id FROM items WHERE chat_id = ?1 AND message_id = ?2",
            rusqlite::params![item.telegram.chat_id, item.telegram.message_id],
            |row| row.get::<_, i64>(0),
        )
        .map(ItemId)
        .map_err(map_sqlite)
    }

    fn get_item(&self, id: ItemId) -> Result<Option<Item>> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id, source, space, kind, text, chat_id, message_id, telegram_extra, created_at
             FROM items WHERE id = ?1",
            [id.0],
            mapping::row_to_item,
        )
        .optional()
        .map_err(map_sqlite)?
        .transpose()
    }

    fn fetch_unprocessed(&self, agent_slug: &str, space: &str, limit: u32) -> Result<Vec<Item>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT i.id, i.source, i.space, i.kind, i.text,
                        i.chat_id, i.message_id, i.telegram_extra, i.created_at
                 FROM items i
                 WHERE i.space = ?1
                   AND NOT EXISTS (
                       SELECT 1 FROM processed_marks m
                       WHERE m.item_id = i.id AND m.agent_slug = ?2
                   )
                 ORDER BY i.id ASC
                 LIMIT ?3",
            )
            .map_err(map_sqlite)?;
        let rows = stmt
            .query_map(
                rusqlite::params![space, agent_slug, limit],
                mapping::row_to_item,
            )
            .map_err(map_sqlite)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(map_sqlite)??);
        }
        Ok(items)
    }

    fn mark_processed(&self, agent_slug: &str, item_id: ItemId) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO processed_marks (agent_slug, item_id, processed_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(agent_slug, item_id) DO NOTHING",
            rusqlite::params![agent_slug, item_id.0, now_unix()],
        )
        .map_err(map_sqlite)?;
        Ok(())
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn map_sqlite(err: rusqlite::Error) -> Error {
    Error::Storage(err.to_string())
}

#[cfg(test)]
mod tests;
