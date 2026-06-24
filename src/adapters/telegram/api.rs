//! Telegram Bot API DTOs (only the fields we care about).
//!
//! The schema is the public Bot API contract; unknown fields are ignored.
//! Optional fields appear because most non-text messages omit `text`/`caption`/
//! `from`. Anything we don't read is dropped during deserialization.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Response<T> {
    pub ok: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub result: Option<T>,
}

#[derive(Debug, Deserialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub message_id: i64,
    pub date: i64,
    pub chat: Chat,
    #[serde(default)]
    pub from: Option<User>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub photo: Option<serde_json::Value>,
    #[serde(default)]
    pub video: Option<serde_json::Value>,
    #[serde(default)]
    pub document: Option<serde_json::Value>,
    #[serde(default)]
    pub audio: Option<serde_json::Value>,
    #[serde(default)]
    pub voice: Option<Voice>,
    #[serde(default)]
    pub video_note: Option<serde_json::Value>,
    #[serde(default)]
    pub sticker: Option<serde_json::Value>,
    #[serde(default)]
    pub animation: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct Chat {
    pub id: i64,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Voice {
    pub file_id: String,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct FileInfo {
    pub file_path: String,
    #[serde(default)]
    pub file_size: Option<i64>,
}
