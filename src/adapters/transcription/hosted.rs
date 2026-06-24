//! OpenAI-shaped hosted transcription adapter.
//!
//! OpenAI and Mistral expose the same multipart `audio/transcriptions` shape:
//! `Authorization: Bearer <token>` + form parts `model=<id>`, `file=<bytes>`,
//! and optional `language=<code>`; the response is JSON `{ "text": "..." }`.
//! One adapter parameterised by URL+model covers both.

use async_trait::async_trait;
use bytes::Bytes;
use reqwest::{
    multipart::{Form, Part},
    Client,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use crate::domain::error::{Error, Result};
use crate::domain::ports::Transcription;

const OPENAI_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
const OPENAI_MODEL: &str = "whisper-1";
const MISTRAL_URL: &str = "https://api.mistral.ai/v1/audio/transcriptions";
const MISTRAL_MODEL: &str = "voxtral-mini-2602";

/// Hint sent to the provider so it doesn't auto-detect the wrong language for
/// short clips. The bot's primary audience is Russian-speaking; English is
/// recognised regardless because both providers fall back to multilingual
/// decoding when the audio doesn't match the hint.
const LANGUAGE: &str = "ru";

pub struct Hosted {
    endpoint: &'static str,
    model: &'static str,
    token: SecretString,
    http: Client,
}

impl std::fmt::Debug for Hosted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hosted")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl Hosted {
    pub fn openai(http: Client, token: SecretString) -> Self {
        Self::new(OPENAI_URL, OPENAI_MODEL, http, token)
    }

    pub fn mistral(http: Client, token: SecretString) -> Self {
        Self::new(MISTRAL_URL, MISTRAL_MODEL, http, token)
    }

    fn new(endpoint: &'static str, model: &'static str, http: Client, token: SecretString) -> Self {
        Hosted {
            endpoint,
            model,
            token,
            http,
        }
    }
}

#[async_trait]
impl Transcription for Hosted {
    async fn transcribe(&self, audio: Bytes, filename: &str) -> Result<String> {
        let part = Part::stream(audio)
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| Error::Transcription(format!("multipart mime: {e}")))?;
        let form = Form::new()
            .text("model", self.model)
            .text("language", LANGUAGE)
            .part("file", part);

        let resp = self
            .http
            .post(self.endpoint)
            .bearer_auth(self.token.expose_secret())
            .multipart(form)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(Error::Transcription(format!(
                "{} returned {}: {}",
                self.endpoint,
                status,
                truncate(&body, 240)
            )));
        }

        let parsed: TranscriptionResponse = serde_json::from_str(&body)?;
        Ok(parsed.text)
    }
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let cut = s.char_indices().nth(max).map_or(max, |(i, _)| i);
        format!("{}…", &s[..cut])
    }
}
