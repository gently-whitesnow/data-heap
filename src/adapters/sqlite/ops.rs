//! Synchronous SQL operations used by the async [`Storage`] impl.
//!
//! Each function takes a `&Connection` and runs inside `spawn_blocking` —
//! that's why this module never touches `tokio` or `async_trait`.

use rusqlite::{Connection, OptionalExtension};

use crate::domain::error::Result;
use crate::domain::item::{Item, ItemId, NewItem};
use crate::domain::source::{Source, Space};

use super::mapping;

pub fn upsert_source(conn: &Connection, source: &Source) -> Result<()> {
    conn.execute(
        "INSERT INTO sources (slug, space)
         VALUES (?1, ?2)
         ON CONFLICT(slug) DO UPDATE SET space = excluded.space",
        rusqlite::params![source.slug, source.space.as_str()],
    )?;
    Ok(())
}

pub fn list_sources(conn: &Connection) -> Result<Vec<Source>> {
    let mut stmt = conn.prepare("SELECT slug, space FROM sources ORDER BY slug")?;
    let rows = stmt.query_map([], |row| {
        Ok(Source {
            slug: row.get(0)?,
            space: Space::new(row.get::<_, String>(1)?),
        })
    })?;
    let mut sources = Vec::new();
    for row in rows {
        sources.push(row?);
    }
    Ok(sources)
}

pub fn insert_item(conn: &Connection, item: &NewItem, now: i64) -> Result<ItemId> {
    let extra = mapping::extra_json(&item.telegram)?;
    // Dedup on the Telegram message address: a repeated polling update is a
    // no-op that returns the id of the already-stored item.
    let inserted = conn.execute(
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
            now,
        ],
    )?;
    if inserted > 0 {
        return Ok(ItemId(conn.last_insert_rowid()));
    }
    let id = conn.query_row(
        "SELECT id FROM items WHERE chat_id = ?1 AND message_id = ?2",
        rusqlite::params![item.telegram.chat_id, item.telegram.message_id],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(ItemId(id))
}

pub fn insert_items(conn: &mut Connection, items: &[NewItem], now: i64) -> Result<Vec<ItemId>> {
    let tx = conn.transaction()?;
    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        ids.push(insert_item(&tx, item, now)?);
    }
    tx.commit()?;
    Ok(ids)
}

pub fn get_item(conn: &Connection, id: ItemId) -> Result<Option<Item>> {
    conn.query_row(
        "SELECT id, source, space, kind, text, chat_id, message_id, telegram_extra, created_at
         FROM items WHERE id = ?1 AND deleted_at IS NULL",
        [id.0],
        mapping::row_to_item,
    )
    .optional()?
    .transpose()
}

pub fn fetch_unprocessed(
    conn: &Connection,
    agent_slug: &str,
    space: &str,
    limit: u32,
) -> Result<Vec<Item>> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.source, i.space, i.kind, i.text,
                i.chat_id, i.message_id, i.telegram_extra, i.created_at
         FROM items i
         WHERE i.space = ?1
           AND i.deleted_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM processed_marks m
               WHERE m.item_id = i.id AND m.agent_slug = ?2
           )
         ORDER BY i.id ASC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![space, agent_slug, limit],
        mapping::row_to_item,
    )?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row??);
    }
    Ok(items)
}

pub fn mark_processed(conn: &Connection, agent_slug: &str, item_id: ItemId, now: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO processed_marks (agent_slug, item_id, processed_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(agent_slug, item_id) DO NOTHING",
        rusqlite::params![agent_slug, item_id.0, now],
    )?;
    Ok(())
}

pub fn delete_item(conn: &Connection, item_id: ItemId, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE items SET deleted_at = ?1
         WHERE id = ?2 AND deleted_at IS NULL",
        rusqlite::params![now, item_id.0],
    )?;
    Ok(())
}
