//! Telegram long-polling adapter implementing the
//! [`IngestionSource`](crate::domain::ports::IngestionSource) port.
//!
//! One adapter instance = one bot = one space (pinned at construction). The
//! adapter owns no HTTP client of its own — it borrows the daemon-wide one
//! — holds the `update_id` offset between polls, downloads and transcribes
//! voice messages through an optional [`Transcription`] port, replies in chat
//! to unsupported or failed message kinds, and handles the slice-3.5
//! save-confirmation + inline-delete callback flow against a [`Storage`]
//! reference.

mod api;
mod confirm;
mod parse;
mod voice;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;

use crate::domain::error::{Error, Result};
use crate::domain::item::ItemId;
use crate::domain::ports::{
    IncomingMessage, IncomingPayload, IngestionSource, Storage, Transcription,
};
use crate::domain::source::Space;

use self::api::{Message, Response, Update};
use self::parse::{parse, MessageSkeleton, Parsed};

/// Telegram long-poll timeout, seconds. Server holds the request up to this
/// long when there are no updates; keep below typical HTTP/proxy idle limits.
pub(crate) const LONG_POLL_SECS: u64 = 25;

/// Telegram Bot API base. Pulled out so tests can point at a mock server.
const DEFAULT_BASE_URL: &str = "https://api.telegram.org";

/// Voice-not-configured reply: the bot is alive but its source has
/// `transcription_provider = "none"`, so we tell the user instead of silently
/// dropping the message.
const VOICE_DISABLED: &str =
    "Транскрибация голосовых для этого бота отключена в конфиге (provider = \"none\").";

/// Polling result with optional cool-down hint from the server (429
/// `retry_after`). The polling loop honours `retry_after` before its own
/// backoff so we don't keep banging on a rate-limited endpoint.
#[derive(Debug)]
pub enum PollOutcome {
    Batch(Vec<IncomingMessage>),
    RetryAfter(Duration),
}

pub struct TelegramSource {
    slug: String,
    space: Space,
    token: SecretString,
    base_url: String,
    http: Client,
    offset: i64,
    transcription: Option<Arc<dyn Transcription>>,
    storage: Arc<dyn Storage>,
}

impl std::fmt::Debug for TelegramSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramSource")
            .field("slug", &self.slug)
            .field("space", &self.space)
            .field("base_url", &self.base_url)
            .field("token", &"<redacted>")
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl TelegramSource {
    pub fn new(
        slug: impl Into<String>,
        space: Space,
        token: SecretString,
        http: Client,
        storage: Arc<dyn Storage>,
    ) -> Self {
        Self::with_base_url(slug, space, token, DEFAULT_BASE_URL, None, http, storage)
    }

    pub fn with_transcription(
        slug: impl Into<String>,
        space: Space,
        token: SecretString,
        transcription: Option<Arc<dyn Transcription>>,
        http: Client,
        storage: Arc<dyn Storage>,
    ) -> Self {
        Self::with_base_url(
            slug,
            space,
            token,
            DEFAULT_BASE_URL,
            transcription,
            http,
            storage,
        )
    }

    pub fn with_base_url(
        slug: impl Into<String>,
        space: Space,
        token: SecretString,
        base_url: impl Into<String>,
        transcription: Option<Arc<dyn Transcription>>,
        http: Client,
        storage: Arc<dyn Storage>,
    ) -> Self {
        TelegramSource {
            slug: slug.into(),
            space,
            token,
            base_url: base_url.into(),
            http,
            offset: 0,
            transcription,
            storage,
        }
    }

    fn method_url(&self, method: &str) -> String {
        format!(
            "{}/bot{}/{}",
            self.base_url,
            self.token.expose_secret(),
            method
        )
    }

    async fn get_updates(&mut self) -> Result<PollOutcome> {
        let body = json!({
            "offset": self.offset,
            "timeout": LONG_POLL_SECS,
            "allowed_updates": ["message", "callback_query"],
        });
        let resp: Response<Vec<Update>> = self
            .http
            .post(self.method_url("getUpdates"))
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        if !resp.ok {
            if let Some(retry) = resp
                .parameters
                .as_ref()
                .and_then(|p| p.retry_after)
                .filter(|s| *s > 0)
            {
                tracing::warn!(
                    source = %self.slug,
                    retry_after = retry,
                    "telegram getUpdates rate-limited"
                );
                return Ok(PollOutcome::RetryAfter(Duration::from_secs(retry)));
            }
            return Err(Error::Telegram(format!(
                "getUpdates: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            )));
        }
        Ok(PollOutcome::Batch(
            self.handle_updates(resp.result.unwrap_or_default()).await,
        ))
    }

    async fn handle_updates(&mut self, updates: Vec<Update>) -> Vec<IncomingMessage> {
        let mut out = Vec::new();
        for upd in updates {
            // Advance offset even for updates we drop, otherwise getUpdates
            // would redeliver them forever.
            if upd.update_id >= self.offset {
                self.offset = upd.update_id + 1;
            }
            if let Some(cb) = upd.callback_query {
                if let Err(e) = confirm::handle_delete_callback(
                    &self.http,
                    &self.base_url,
                    &self.token,
                    self.storage.as_ref(),
                    &cb,
                )
                .await
                {
                    tracing::warn!(source = %self.slug, error = %e, "callback_query handling failed");
                }
                continue;
            }
            let Some(msg) = upd.message else { continue };
            match parse(&msg) {
                Parsed::Incoming(im) => out.push(im),
                Parsed::Voice {
                    file_id,
                    mime_type,
                    skeleton,
                } => {
                    if let Some(im) = self
                        .handle_voice(&msg, &file_id, mime_type.as_deref(), skeleton)
                        .await
                    {
                        out.push(im);
                    }
                }
                Parsed::Unsupported(reason) => {
                    if let Err(e) = self.send_reply(&msg, reason).await {
                        tracing::warn!(source = %self.slug, error = %e, "reply failed");
                    }
                }
            }
        }
        out
    }

    async fn send_reply(&self, msg: &Message, text: &str) -> Result<()> {
        let body = json!({
            "chat_id": msg.chat.id,
            "text": text,
            "reply_parameters": { "message_id": msg.message_id },
        });
        let resp: Response<serde_json::Value> = self
            .http
            .post(self.method_url("sendMessage"))
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        if !resp.ok {
            // Don't fail the whole poll because a single reply bounced (user
            // blocked the bot, etc.); the dedup index will keep us idempotent
            // on the next pass.
            tracing::warn!(
                source = %self.slug,
                chat_id = msg.chat.id,
                error = %resp.description.unwrap_or_default(),
                "telegram sendMessage rejected"
            );
        }
        Ok(())
    }

    async fn handle_voice(
        &self,
        msg: &Message,
        file_id: &str,
        mime_type: Option<&str>,
        skeleton: MessageSkeleton,
    ) -> Option<IncomingMessage> {
        let Some(transcription) = self.transcription.as_ref() else {
            if let Err(e) = self.send_reply(msg, VOICE_DISABLED).await {
                tracing::warn!(source = %self.slug, error = %e, "voice-disabled reply failed");
            }
            return None;
        };
        match voice::transcribe_voice(
            &self.http,
            &self.base_url,
            &self.token,
            file_id,
            mime_type,
            transcription.as_ref(),
        )
        .await
        {
            Ok(transcript) => Some(skeleton.into_incoming(IncomingPayload::Voice(transcript))),
            Err(err) => {
                tracing::warn!(
                    source = %self.slug,
                    chat_id = msg.chat.id,
                    error = %err,
                    "voice transcription failed"
                );
                let reply = format!("Не удалось расшифровать голосовое: {err}");
                if let Err(e) = self.send_reply(msg, &reply).await {
                    tracing::warn!(source = %self.slug, error = %e, "voice-failed reply failed");
                }
                None
            }
        }
    }

    /// Poll variant exposing the rate-limit hint to the caller.
    pub async fn poll_outcome(&mut self) -> Result<PollOutcome> {
        self.get_updates().await
    }
}

#[async_trait]
impl IngestionSource for TelegramSource {
    fn slug(&self) -> &str {
        &self.slug
    }

    fn space(&self) -> &Space {
        &self.space
    }

    async fn poll(&mut self) -> Result<Vec<IncomingMessage>> {
        match self.poll_outcome().await? {
            PollOutcome::Batch(b) => Ok(b),
            // Surface the rate-limit as an empty batch — the polling loop in
            // the daemon uses `poll_outcome` directly when wired by config,
            // but generic consumers still get backoff via Result<Vec<…>>.
            PollOutcome::RetryAfter(d) => {
                tokio::time::sleep(d).await;
                Ok(Vec::new())
            }
        }
    }

    async fn confirm_saved(&self, message: &IncomingMessage, item_id: ItemId) -> Result<()> {
        let saved_text = match &message.payload {
            IncomingPayload::Text(t) | IncomingPayload::Caption(t) | IncomingPayload::Voice(t) => {
                t.as_str()
            }
        };
        if let Err(e) = confirm::send_confirmation(
            &self.http,
            &self.base_url,
            &self.token,
            message.chat_id,
            message.message_id,
            saved_text,
            item_id,
        )
        .await
        {
            tracing::warn!(source = %self.slug, error = %e, "confirmation reply failed");
        }
        Ok(())
    }
}
