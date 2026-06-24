//! OpenAI-shaped hosted transcription adapter.
//!
//! OpenAI and Mistral expose the same multipart `audio/transcriptions` shape:
//! `Authorization: Bearer <token>` + form parts `model=<id>`, `file=<bytes>`,
//! and optional `language=<code>`; the response is JSON `{ "text": "..." }`.
//! One adapter parameterised by URL+model covers both.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{
    multipart::{Form, Part},
    Client,
};
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

/// Transcription request/response timeout. Voice messages are short (≤20 MB
/// per Telegram limit), but providers can be slow on cold starts.
const HTTP_TIMEOUT_SECS: u64 = 60;

pub struct Hosted {
    endpoint: &'static str,
    model: &'static str,
    token: String,
    http: Client,
}

impl Hosted {
    pub fn openai(token: impl Into<String>) -> Result<Self> {
        Self::new(OPENAI_URL, OPENAI_MODEL, token)
    }

    pub fn mistral(token: impl Into<String>) -> Result<Self> {
        Self::new(MISTRAL_URL, MISTRAL_MODEL, token)
    }

    fn new(endpoint: &'static str, model: &'static str, token: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| Error::Transcription(format!("http client init: {e}")))?;
        Ok(Hosted {
            endpoint,
            model,
            token: token.into(),
            http,
        })
    }
}

#[async_trait]
impl Transcription for Hosted {
    async fn transcribe(&self, audio: &[u8], filename: &str) -> Result<String> {
        let part = Part::bytes(audio.to_vec())
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
            .bearer_auth(&self.token)
            .multipart(form)
            .send()
            .await
            .map_err(|e| Error::Transcription(format!("request failed: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Error::Transcription(format!("read response: {e}")))?;
        if !status.is_success() {
            return Err(Error::Transcription(format!(
                "{} returned {}: {}",
                self.endpoint,
                status,
                truncate(&body, 240)
            )));
        }

        let parsed: TranscriptionResponse = serde_json::from_str(&body)
            .map_err(|e| Error::Transcription(format!("decode response: {e}")))?;
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
