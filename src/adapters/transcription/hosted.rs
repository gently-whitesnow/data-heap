//! OpenAI-shaped hosted transcription adapter.
//!
//! OpenAI and Mistral expose the same multipart `audio/transcriptions` shape:
//! `Authorization: Bearer <token>` + form parts `model=<id>` and
//! `file=<audio bytes>`; the response is JSON `{ "text": "..." }`. One adapter
//! parameterised by URL+model covers both — and any future provider that
//! mimics this contract.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{
    multipart::{Form, Part},
    Client,
};
use serde::Deserialize;

use crate::domain::error::{Error, Result};
use crate::domain::ports::Transcription;

pub const OPENAI_DEFAULT_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
pub const OPENAI_DEFAULT_MODEL: &str = "whisper-1";
pub const MISTRAL_DEFAULT_URL: &str = "https://api.mistral.ai/v1/audio/transcriptions";
pub const MISTRAL_DEFAULT_MODEL: &str = "voxtral-mini-latest";

/// Transcription request/response timeout. Voice messages are short (≤20 MB
/// per Telegram limit), but providers can be slow on cold starts.
const HTTP_TIMEOUT_SECS: u64 = 60;

pub struct Hosted {
    endpoint: String,
    token: String,
    model: String,
    http: Client,
}

impl Hosted {
    pub fn openai(token: impl Into<String>, endpoint: impl Into<String>) -> Result<Self> {
        Self::new(endpoint, token, OPENAI_DEFAULT_MODEL)
    }

    pub fn mistral(token: impl Into<String>, endpoint: impl Into<String>) -> Result<Self> {
        Self::new(endpoint, token, MISTRAL_DEFAULT_MODEL)
    }

    fn new(
        endpoint: impl Into<String>,
        token: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| Error::Transcription(format!("http client init: {e}")))?;
        Ok(Hosted {
            endpoint: endpoint.into(),
            token: token.into(),
            model: model.into(),
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
            .text("model", self.model.clone())
            .part("file", part);

        let resp = self
            .http
            .post(&self.endpoint)
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
        let cut = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(max);
        format!("{}…", &s[..cut])
    }
}
