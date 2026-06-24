//! Telegram long-polling adapter implementing the
//! [`IngestionSource`](crate::domain::ports::IngestionSource) port.
//!
//! One adapter instance = one bot = one space (pinned at construction). The
//! adapter is fully self-contained: it owns the HTTP client, holds the
//! `update_id` offset between polls, and replies in chat to unsupported message
//! kinds — the caller only sees text-bearing messages.

mod api;
mod parse;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use crate::domain::error::{Error, Result};
use crate::domain::ports::{IncomingMessage, IngestionSource};
use crate::domain::source::Space;

use self::api::{Message, Response, Update};
use self::parse::{parse, Parsed};

/// Telegram long-poll timeout, seconds. Server holds the request up to this
/// long when there are no updates; keep below typical HTTP/proxy idle limits.
const LONG_POLL_SECS: u64 = 25;

/// HTTP request timeout — long-poll seconds plus headroom for the response.
const HTTP_TIMEOUT_SECS: u64 = LONG_POLL_SECS + 10;

/// Telegram Bot API base. Pulled out so tests can point at a mock server.
const DEFAULT_BASE_URL: &str = "https://api.telegram.org";

pub struct TelegramSource {
    slug: String,
    space: Space,
    token: String,
    base_url: String,
    http: Client,
    offset: i64,
}

impl TelegramSource {
    pub fn new(slug: impl Into<String>, space: Space, token: impl Into<String>) -> Result<Self> {
        Self::with_base_url(slug, space, token, DEFAULT_BASE_URL)
    }

    pub fn with_base_url(
        slug: impl Into<String>,
        space: Space,
        token: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| Error::Storage(format!("http client init: {e}")))?;
        Ok(TelegramSource {
            slug: slug.into(),
            space,
            token: token.into(),
            base_url: base_url.into(),
            http,
            offset: 0,
        })
    }

    fn method_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.base_url, self.token, method)
    }

    async fn get_updates(&self) -> Result<Vec<Update>> {
        let body = json!({
            "offset": self.offset,
            "timeout": LONG_POLL_SECS,
            "allowed_updates": ["message"],
        });
        let resp: Response<Vec<Update>> = self
            .http
            .post(self.method_url("getUpdates"))
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Storage(format!("telegram getUpdates: {e}")))?
            .json()
            .await
            .map_err(|e| Error::Storage(format!("telegram getUpdates decode: {e}")))?;
        if !resp.ok {
            return Err(Error::Storage(format!(
                "telegram getUpdates: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            )));
        }
        Ok(resp.result.unwrap_or_default())
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
            .await
            .map_err(|e| Error::Storage(format!("telegram sendMessage: {e}")))?
            .json()
            .await
            .map_err(|e| Error::Storage(format!("telegram sendMessage decode: {e}")))?;
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
        let updates = self.get_updates().await?;
        let mut out = Vec::new();
        for upd in updates {
            // Advance offset even for updates we drop, otherwise getUpdates
            // would redeliver them forever.
            if upd.update_id >= self.offset {
                self.offset = upd.update_id + 1;
            }
            let Some(msg) = upd.message else { continue };
            match parse(&msg) {
                Parsed::Incoming(im) => out.push(im),
                Parsed::Unsupported(reason) => {
                    if let Err(e) = self.send_reply(&msg, reason).await {
                        tracing::warn!(source = %self.slug, error = %e, "reply failed");
                    }
                }
            }
        }
        Ok(out)
    }
}
