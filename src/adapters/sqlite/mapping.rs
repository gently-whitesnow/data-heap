use rusqlite::Row;

use crate::domain::error::{Error, Result};
use crate::domain::item::{Item, ItemId, ItemKind, TelegramMetadata};
use crate::domain::source::Space;

/// Map an `items` row to a domain [`Item`]. The outer `rusqlite::Result`
/// reports column-access failures; the inner [`Result`] reports decode failures
/// (unknown kind, malformed metadata JSON) so they surface as domain errors.
pub fn row_to_item(row: &Row<'_>) -> rusqlite::Result<Result<Item>> {
    let id: i64 = row.get(0)?;
    let source: String = row.get(1)?;
    let space: String = row.get(2)?;
    let kind_raw: String = row.get(3)?;
    let text: String = row.get(4)?;
    let metadata_raw: String = row.get(5)?;
    let created_at: i64 = row.get(6)?;

    Ok(decode(
        id,
        source,
        space,
        kind_raw,
        text,
        metadata_raw,
        created_at,
    ))
}

fn decode(
    id: i64,
    source: String,
    space: String,
    kind_raw: String,
    text: String,
    metadata_raw: String,
    created_at: i64,
) -> Result<Item> {
    let kind = ItemKind::parse(&kind_raw)
        .ok_or_else(|| Error::Storage(format!("unknown item kind '{kind_raw}'")))?;
    let telegram: TelegramMetadata = serde_json::from_str(&metadata_raw)
        .map_err(|e| Error::Serialization(format!("item {id} metadata: {e}")))?;
    Ok(Item {
        id: ItemId(id),
        source,
        space: Space::new(space),
        kind,
        text,
        telegram,
        created_at,
    })
}
