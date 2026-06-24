//! TOML configuration: daemon parameters and the list of ingestion sources.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use crate::domain::error::{Error, Result};
use crate::domain::source::{Source, Space, TranscriptionProvider};

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_database_path")]
    pub database_path: PathBuf,
    #[serde(default = "default_http_addr")]
    pub http_addr: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        DaemonConfig {
            database_path: default_database_path(),
            http_addr: default_http_addr(),
        }
    }
}

/// One Telegram bot pinned to one space, with its transcription backend.
/// Tokens are wrapped in [`SecretString`] so they can never leak through
/// `{:?}` formatting or serde re-serialization.
#[derive(Deserialize)]
pub struct SourceConfig {
    pub slug: String,
    pub space: String,
    #[serde(deserialize_with = "deserialize_secret")]
    pub bot_token: SecretString,
    #[serde(default)]
    pub transcription_provider: TranscriptionProvider,
    #[serde(default, deserialize_with = "deserialize_optional_secret")]
    pub transcription_token: Option<SecretString>,
    /// Telegram `from.id` values permitted to write to this bot. Empty/missing
    /// list is fail-closed: every incoming message is silently dropped.
    #[serde(default)]
    pub allowed_user_ids: Vec<i64>,
}

impl std::fmt::Debug for SourceConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceConfig")
            .field("slug", &self.slug)
            .field("space", &self.space)
            .field("bot_token", &"<redacted>")
            .field("transcription_provider", &self.transcription_provider)
            .field(
                "transcription_token",
                &self.transcription_token.as_ref().map(|_| "<redacted>"),
            )
            .field("allowed_user_ids", &self.allowed_user_ids)
            .finish()
    }
}

impl SourceConfig {
    /// Project the persisted view of this source (slug→space binding only).
    pub fn to_source(&self) -> Source {
        Source {
            slug: self.slug.clone(),
            space: Space::new(self.space.clone()),
        }
    }
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("cannot read {}: {e}", path.display())))?;
        Self::from_toml(&raw)
    }

    pub fn from_toml(raw: &str) -> Result<Self> {
        let config: Config = toml::from_str(raw)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        let mut slugs = HashSet::new();
        for src in &self.sources {
            if src.slug.trim().is_empty() {
                return Err(Error::Config("source slug must not be empty".into()));
            }
            if src.space.trim().is_empty() {
                return Err(Error::Config(format!(
                    "source '{}' has empty space",
                    src.slug
                )));
            }
            if src.bot_token.expose_secret().trim().is_empty() {
                return Err(Error::Config(format!(
                    "source '{}' has empty bot_token",
                    src.slug
                )));
            }
            let needs_token = matches!(
                src.transcription_provider,
                TranscriptionProvider::Mistral | TranscriptionProvider::Openai
            );
            if needs_token
                && src
                    .transcription_token
                    .as_ref()
                    .map_or(true, |s| s.expose_secret().trim().is_empty())
            {
                return Err(Error::Config(format!(
                    "source '{}' uses provider '{}' but has no transcription_token",
                    src.slug,
                    src.transcription_provider.as_str()
                )));
            }
            if !slugs.insert(src.slug.clone()) {
                return Err(Error::Config(format!(
                    "duplicate source slug '{}'",
                    src.slug
                )));
            }
            if src.allowed_user_ids.is_empty() {
                tracing::warn!(
                    source = %src.slug,
                    "allowed_user_ids is empty; every incoming message will be silently dropped"
                );
            }
        }
        Ok(())
    }
}

fn default_database_path() -> PathBuf {
    PathBuf::from("data-heap.sqlite")
}

fn default_http_addr() -> String {
    "127.0.0.1:8080".to_string()
}

fn deserialize_secret<'de, D>(deserializer: D) -> std::result::Result<SecretString, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(SecretString::from(s))
}

fn deserialize_optional_secret<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<SecretString>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.map(SecretString::from))
}

#[cfg(test)]
mod tests;
