//! Local OpenAI-compatible Whisper server. Same multipart contract as the
//! hosted adapter, but with no auth header and a config-supplied URL — the
//! user points it at their own `whisper.cpp` / `faster-whisper-server`
//! instance.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{
    multipart::{Form, Part},
    Client,
};
use serde::Deserialize;

use crate::domain::error::{Error, Result};
use crate::domain::ports::Transcription;

const HTTP_TIMEOUT_SECS: u64 = 120;
const DEFAULT_MODEL: &str = "whisper-1";

pub struct LocalWhisper {
    endpoint: String,
    http: Client,
}

impl LocalWhisper {
    pub fn new(endpoint: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| Error::Transcription(format!("http client init: {e}")))?;
        Ok(LocalWhisper {
            endpoint: endpoint.into(),
            http,
        })
    }
}

#[async_trait]
impl Transcription for LocalWhisper {
    async fn transcribe(&self, audio: &[u8], filename: &str) -> Result<String> {
        let part = Part::bytes(audio.to_vec())
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| Error::Transcription(format!("multipart mime: {e}")))?;
        let form = Form::new().text("model", DEFAULT_MODEL).part("file", part);

        let resp = self
            .http
            .post(&self.endpoint)
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
                self.endpoint, status, body
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
