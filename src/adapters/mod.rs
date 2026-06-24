//! Infrastructure adapters implementing domain ports.
//!
//! SQLite implements [`Storage`](crate::domain::ports::Storage); Telegram
//! implements [`IngestionSource`](crate::domain::ports::IngestionSource).
//! Transcription arrives in slice 3.

pub mod sqlite;
pub mod telegram;

pub use sqlite::SqliteStorage;
pub use telegram::TelegramSource;
