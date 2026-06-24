use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::source::Space;

/// SQLite rowid of a stored item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemId(pub i64);

impl std::fmt::Display for ItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What kind of text a stored item carries. The service only ever keeps text;
/// the kind records where that text came from in the original message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Plain text message.
    Text,
    /// A link, stored verbatim as text (never fetched).
    Link,
    /// Caption attached to a non-text message (photo/video/document).
    Caption,
    /// Transcript of a voice message.
    Voice,
}

impl ItemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemKind::Text => "text",
            ItemKind::Link => "link",
            ItemKind::Caption => "caption",
            ItemKind::Voice => "voice",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownItemKind(pub String);

impl std::fmt::Display for UnknownItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown item kind '{}'", self.0)
    }
}

impl std::error::Error for UnknownItemKind {}

impl FromStr for ItemKind {
    type Err = UnknownItemKind;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "text" => Ok(ItemKind::Text),
            "link" => Ok(ItemKind::Link),
            "caption" => Ok(ItemKind::Caption),
            "voice" => Ok(ItemKind::Voice),
            other => Err(UnknownItemKind(other.to_string())),
        }
    }
}

/// Telegram-side provenance kept alongside the text. Storage promotes
/// `(chat_id, message_id)` to indexed columns (the message dedup key) and keeps
/// the rest in a JSON blob so the shape can grow without a schema migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramMetadata {
    pub chat_id: i64,
    pub message_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Telegram message date, Unix seconds.
    pub date: i64,
}

/// An item awaiting persistence. `created_at` is assigned by storage on insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewItem {
    pub source: String,
    pub space: Space,
    pub kind: ItemKind,
    pub text: String,
    pub telegram: TelegramMetadata,
}

/// A persisted item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: ItemId,
    pub source: String,
    pub space: Space,
    pub kind: ItemKind,
    pub text: String,
    pub telegram: TelegramMetadata,
    /// Storage timestamp, Unix seconds.
    pub created_at: i64,
}
