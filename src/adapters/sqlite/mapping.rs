use rusqlite::Row;
use serde::{Deserialize, Serialize};

use crate::domain::error::{Error, Result};
use crate::domain::item::{Item, ItemId, ItemKind, TelegramMetadata};
use crate::domain::source::Space;

/// JSON-persisted part of [`TelegramMetadata`]: everything except the
/// `(chat_id, message_id)` dedup key, which lives in dedicated columns.
#[derive(Serialize, Deserialize)]
struct TelegramExtra {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    date: i64,
}

/// Serialize the JSON-stored portion of the metadata.
pub fn extra_json(meta: &TelegramMetadata) -> Result<String> {
    let extra = TelegramExtra {
        user_id: meta.user_id,
        username: meta.username.clone(),
        date: meta.date,
    };
    serde_json::to_string(&extra).map_err(|e| Error::Serialization(e.to_string()))
}

/// Map an `items` row to a domain [`Item`]. The outer `rusqlite::Result`
/// reports column-access failures; the inner [`Result`] reports decode failures
/// (unknown kind, malformed metadata JSON) so they surface as domain errors.
pub fn row_to_item(row: &Row<'_>) -> rusqlite::Result<Result<Item>> {
    let id: i64 = row.get(0)?;
    let source: String = row.get(1)?;
    let space: String = row.get(2)?;
    let kind_raw: String = row.get(3)?;
    let text: String = row.get(4)?;
    let chat_id: i64 = row.get(5)?;
    let message_id: i64 = row.get(6)?;
    let extra_raw: String = row.get(7)?;
    let created_at: i64 = row.get(8)?;

    Ok(decode(
        id, source, space, kind_raw, text, chat_id, message_id, extra_raw, created_at,
    ))
}

#[allow(clippy::too_many_arguments)]
fn decode(
    id: i64,
    source: String,
    space: String,
    kind_raw: String,
    text: String,
    chat_id: i64,
    message_id: i64,
    extra_raw: String,
    created_at: i64,
) -> Result<Item> {
    let kind = kind_raw
        .parse::<ItemKind>()
        .map_err(|e| Error::Storage(e.to_string()))?;
    let extra: TelegramExtra = serde_json::from_str(&extra_raw)
        .map_err(|e| Error::Serialization(format!("item {id} metadata: {e}")))?;
    Ok(Item {
        id: ItemId(id),
        source,
        space: Space::new(space),
        kind,
        text,
        telegram: TelegramMetadata {
            chat_id,
            message_id,
            user_id: extra.user_id,
            username: extra.username,
            date: extra.date,
        },
        created_at,
    })
}
