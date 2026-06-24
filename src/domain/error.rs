use thiserror::Error;

/// Errors crossing domain ports. Adapters map their infrastructure failures
/// (rusqlite, serde, io) into these variants so the domain stays agnostic.
#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("transcription error: {0}")]
    Transcription(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
