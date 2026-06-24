//! TOML configuration: daemon parameters and the list of ingestion sources.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::error::{Error, Result};
use crate::domain::source::{Source, Space, TranscriptionProvider};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_database_path")]
    pub database_path: PathBuf,
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_http_addr")]
    pub http_addr: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        DaemonConfig {
            database_path: default_database_path(),
            poll_interval_secs: default_poll_interval_secs(),
            http_addr: default_http_addr(),
        }
    }
}

/// One Telegram bot pinned to one space, with its transcription backend.
/// Holds secrets (`bot_token`, `transcription_token`) that never reach storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceConfig {
    pub slug: String,
    pub space: String,
    pub bot_token: String,
    #[serde(default)]
    pub transcription_provider: TranscriptionProvider,
    #[serde(default)]
    pub transcription_token: Option<String>,
    /// Override the provider's HTTP endpoint. Required for `local_whisper`
    /// (point at the local STT server); optional for hosted providers, mainly
    /// useful in tests.
    #[serde(default)]
    pub transcription_url: Option<String>,
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
        let config: Config =
            toml::from_str(raw).map_err(|e| Error::Config(format!("invalid TOML: {e}")))?;
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
            if src.bot_token.trim().is_empty() {
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
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            {
                return Err(Error::Config(format!(
                    "source '{}' uses provider '{}' but has no transcription_token",
                    src.slug,
                    src.transcription_provider.as_str()
                )));
            }
            if matches!(
                src.transcription_provider,
                TranscriptionProvider::LocalWhisper
            ) && src
                .transcription_url
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err(Error::Config(format!(
                    "source '{}' uses provider 'local_whisper' but has no transcription_url",
                    src.slug
                )));
            }
            if !slugs.insert(&src.slug) {
                return Err(Error::Config(format!(
                    "duplicate source slug '{}'",
                    src.slug
                )));
            }
        }
        Ok(())
    }
}

fn default_database_path() -> PathBuf {
    PathBuf::from("data-heap.sqlite")
}

fn default_poll_interval_secs() -> u64 {
    5
}

fn default_http_addr() -> String {
    "127.0.0.1:8080".to_string()
}

#[cfg(test)]
mod tests;
