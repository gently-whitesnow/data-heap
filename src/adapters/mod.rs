//! Infrastructure adapters implementing domain ports.
//!
//! SQLite implements [`Storage`](crate::domain::ports::Storage); Telegram
//! implements [`IngestionSource`](crate::domain::ports::IngestionSource);
//! [`transcription`] implements [`Transcription`](crate::domain::ports::Transcription)
//! via OpenAI-shaped HTTP APIs (OpenAI, Mistral, local Whisper).

pub mod sqlite;
pub mod telegram;
pub mod transcription;

pub use sqlite::SqliteStorage;
pub use telegram::TelegramSource;
