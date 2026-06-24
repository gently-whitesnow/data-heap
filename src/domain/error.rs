use thiserror::Error;

use super::item::UnknownItemKind;

/// Errors crossing domain ports. Adapters lift their infrastructure failures
/// into these variants while preserving the original cause through
/// `std::error::Error::source` (`#[from]` / `#[source]`).
#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("config parse error")]
    ConfigParse(#[from] toml::de::Error),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error("storage decode error: {0}")]
    StorageDecode(String),

    #[error("migration {version} failed")]
    Migration {
        version: i64,
        #[source]
        source: rusqlite::Error,
    },

    #[error("serialization error")]
    Serialization(#[from] serde_json::Error),

    #[error(transparent)]
    Network(#[from] reqwest::Error),

    #[error("telegram API error: {0}")]
    Telegram(String),

    #[error("transcription provider error: {0}")]
    Transcription(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("system clock error")]
    Clock(#[from] std::time::SystemTimeError),

    #[error("task join error")]
    Join(#[from] tokio::task::JoinError),
}

impl From<UnknownItemKind> for Error {
    fn from(value: UnknownItemKind) -> Self {
        Error::StorageDecode(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn sqlite_error_preserves_source_chain() {
        let inner = rusqlite::Connection::open_in_memory()
            .unwrap()
            .prepare("SELECT * FROM does_not_exist")
            .unwrap_err();
        let wrapped: Error = inner.into();
        assert!(
            wrapped.source().is_some(),
            "Error::Sqlite must expose the rusqlite cause"
        );
    }

    #[test]
    fn serialization_error_preserves_source_chain() {
        let bad: serde_json::Error = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let wrapped: Error = bad.into();
        assert!(wrapped.source().is_some());
    }

    #[test]
    fn telegram_error_has_no_inner_cause() {
        let e = Error::Telegram("getUpdates: blocked".into());
        assert!(e.source().is_none(), "Telegram is a self-contained variant");
    }

    #[test]
    fn migration_error_keeps_underlying_sqlite_source() {
        let inner = rusqlite::Connection::open_in_memory()
            .unwrap()
            .prepare("SELECT * FROM does_not_exist")
            .unwrap_err();
        let e = Error::Migration {
            version: 1,
            source: inner,
        };
        assert!(e.source().is_some());
    }
}
