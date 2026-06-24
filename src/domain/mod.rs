//! Domain core: entities and trait-ports, free of infrastructure concerns.

pub mod error;
pub mod item;
pub mod ports;
pub mod source;

pub use error::{Error, Result};
pub use item::{Item, ItemId, ItemKind, NewItem, TelegramMetadata};
pub use ports::{IncomingMessage, IncomingPayload, IngestionSource, Storage, Transcription};
pub use source::{Source, Space, TranscriptionProvider};
