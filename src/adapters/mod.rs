//! Infrastructure adapters implementing domain ports.
//!
//! This slice ships the SQLite [`Storage`](crate::domain::ports::Storage)
//! adapter. Ingestion and transcription adapters arrive in later slices.

pub mod sqlite;

pub use sqlite::SqliteStorage;
