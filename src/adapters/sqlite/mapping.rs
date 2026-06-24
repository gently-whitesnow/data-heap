use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::domain::error::Result;
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
    Ok(serde_json::to_string(&extra)?)
}

/// Typed row mirror of the `items` table — sqlx decodes columns by name and
/// [`into_item`] lifts decoded scalars into the domain [`Item`].
#[derive(FromRow)]
pub struct ItemRow {
    pub id: i64,
    pub source: String,
    pub space: String,
    pub kind: String,
    pub text: String,
    pub chat_id: i64,
    pub message_id: i64,
    pub telegram_extra: String,
    pub created_at: i64,
}

/// Decode a row into a domain [`Item`]. Returns a domain error on malformed
/// `kind` or `telegram_extra` JSON, so callers see a typed failure rather than
/// a panic.
pub fn into_item(row: ItemRow) -> Result<Item> {
    let kind = row.kind.parse::<ItemKind>()?;
    let extra: TelegramExtra = serde_json::from_str(&row.telegram_extra)?;
    Ok(Item {
        id: ItemId(row.id),
        source: row.source,
        space: Space::new(row.space),
        kind,
        text: row.text,
        telegram: TelegramMetadata {
            chat_id: row.chat_id,
            message_id: row.message_id,
            user_id: extra.user_id,
            username: extra.username,
            date: extra.date,
        },
        created_at: row.created_at,
    })
}
