//! Async SQL operations behind the [`Storage`](crate::domain::ports::Storage)
//! impl. Each function takes a `&Pool<Sqlite>` (or a transaction) and runs
//! its statements with `sqlx` — no blocking calls, no `spawn_blocking`.

use sqlx::{Pool, Sqlite, Transaction};

use crate::domain::error::Result;
use crate::domain::item::{Item, ItemId, NewItem};
use crate::domain::source::{Source, Space};

use super::mapping::{self, ItemRow};

pub async fn upsert_source(pool: &Pool<Sqlite>, source: &Source) -> Result<()> {
    sqlx::query(
        "INSERT INTO sources (slug, space)
         VALUES (?1, ?2)
         ON CONFLICT(slug) DO UPDATE SET space = excluded.space",
    )
    .bind(&source.slug)
    .bind(source.space.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_sources(pool: &Pool<Sqlite>) -> Result<Vec<Source>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT slug, space FROM sources ORDER BY slug")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(slug, space)| Source {
            slug,
            space: Space::new(space),
        })
        .collect())
}

pub async fn insert_item(pool: &Pool<Sqlite>, item: &NewItem, now: i64) -> Result<ItemId> {
    let mut tx = pool.begin().await?;
    let id = insert_item_tx(&mut tx, item, now).await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn insert_items(pool: &Pool<Sqlite>, items: &[NewItem], now: i64) -> Result<Vec<ItemId>> {
    let mut tx = pool.begin().await?;
    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        ids.push(insert_item_tx(&mut tx, item, now).await?);
    }
    tx.commit().await?;
    Ok(ids)
}

// A txn-scoped insert keeps the `INSERT … ON CONFLICT DO NOTHING` + dedup
// `SELECT id` atomic, so a concurrent insert on the same telegram message
// cannot slip a different id between the two statements.
async fn insert_item_tx(
    tx: &mut Transaction<'_, Sqlite>,
    item: &NewItem,
    now: i64,
) -> Result<ItemId> {
    let extra = mapping::extra_json(&item.telegram)?;
    let res = sqlx::query(
        "INSERT INTO items
                (source, space, kind, text, chat_id, message_id, telegram_extra, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(chat_id, message_id) DO NOTHING",
    )
    .bind(&item.source)
    .bind(item.space.as_str())
    .bind(item.kind.as_str())
    .bind(&item.text)
    .bind(item.telegram.chat_id)
    .bind(item.telegram.message_id)
    .bind(&extra)
    .bind(now)
    .execute(&mut **tx)
    .await?;

    if res.rows_affected() > 0 {
        return Ok(ItemId(res.last_insert_rowid()));
    }
    let id: i64 = sqlx::query_scalar("SELECT id FROM items WHERE chat_id = ?1 AND message_id = ?2")
        .bind(item.telegram.chat_id)
        .bind(item.telegram.message_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(ItemId(id))
}

pub async fn get_item(pool: &Pool<Sqlite>, id: ItemId) -> Result<Option<Item>> {
    let row: Option<ItemRow> = sqlx::query_as(
        "SELECT id, source, space, kind, text, chat_id, message_id, telegram_extra, created_at
         FROM items WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(id.0)
    .fetch_optional(pool)
    .await?;
    row.map(mapping::into_item).transpose()
}

pub async fn fetch_unprocessed(
    pool: &Pool<Sqlite>,
    agent_slug: &str,
    space: &str,
    limit: u32,
) -> Result<Vec<Item>> {
    let rows: Vec<ItemRow> = sqlx::query_as(
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
    )
    .bind(space)
    .bind(agent_slug)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(mapping::into_item).collect()
}

pub async fn mark_processed(
    pool: &Pool<Sqlite>,
    agent_slug: &str,
    item_id: ItemId,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO processed_marks (agent_slug, item_id, processed_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(agent_slug, item_id) DO NOTHING",
    )
    .bind(agent_slug)
    .bind(item_id.0)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_item(pool: &Pool<Sqlite>, item_id: ItemId, now: i64) -> Result<()> {
    sqlx::query(
        "UPDATE items SET deleted_at = ?1
         WHERE id = ?2 AND deleted_at IS NULL",
    )
    .bind(now)
    .bind(item_id.0)
    .execute(pool)
    .await?;
    Ok(())
}
