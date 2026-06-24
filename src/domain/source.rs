use std::convert::Infallible;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A space groups items by purpose (`expenses`, `thoughts`, `links`, `inbox`, …).
/// It is an opaque slug; the domain imposes no fixed vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Space(String);

impl Space {
    pub fn new(slug: impl Into<String>) -> Self {
        Space(slug.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Space {
    fn from(s: String) -> Self {
        Space(s)
    }
}

impl FromStr for Space {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Space(s.to_string()))
    }
}

impl std::fmt::Display for Space {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Speech-to-text backend bound to a source. `None` disables voice handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionProvider {
    Mistral,
    Openai,
    #[default]
    None,
}

impl TranscriptionProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            TranscriptionProvider::Mistral => "mistral",
            TranscriptionProvider::Openai => "openai",
            TranscriptionProvider::None => "none",
        }
    }
}

/// A registered ingestion source — one Telegram bot pinned to one space.
/// Transcription provider and all secrets live in config only; storage keeps
/// just the source→space binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub slug: String,
    pub space: Space,
}
