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
    Sqlx(#[from] sqlx::Error),

    #[error("storage decode error: {0}")]
    StorageDecode(String),

    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),

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

    #[tokio::test]
    async fn sqlx_error_preserves_source_chain() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        let inner = sqlx::query("SELECT * FROM does_not_exist")
            .execute(&pool)
            .await
            .unwrap_err();
        let wrapped: Error = inner.into();
        assert!(
            wrapped.source().is_some(),
            "Error::Sqlx must expose the sqlx cause"
        );
    }

    #[test]
    fn serialization_error_preserves_source_chain() {
        let bad: serde_json::Error =
            serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let wrapped: Error = bad.into();
        assert!(wrapped.source().is_some());
    }

    #[test]
    fn telegram_error_has_no_inner_cause() {
        let e = Error::Telegram("getUpdates: blocked".into());
        assert!(e.source().is_none(), "Telegram is a self-contained variant");
    }
}
