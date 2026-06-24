use async_trait::async_trait;

use super::error::Result;
use super::item::{Item, ItemId, ItemKind, NewItem, TelegramMetadata};
use super::source::{Source, Space};

/// Persistence port. The domain depends on this trait; adapters (SQLite first)
/// implement it. Object-safe and `Send + Sync` so it can be shared as
/// `Arc<dyn Storage>` across daemon tasks.
pub trait Storage: Send + Sync {
    /// Bring the schema up to the latest version. Idempotent.
    fn migrate(&self) -> Result<()>;

    /// Register a source or update its mutable fields (space, transcription).
    fn upsert_source(&self, source: &Source) -> Result<()>;

    fn list_sources(&self) -> Result<Vec<Source>>;

    /// Persist a new item and return its assigned id.
    fn insert_item(&self, item: &NewItem) -> Result<ItemId>;

    fn get_item(&self, id: ItemId) -> Result<Option<Item>>;

    /// Items in `space` not yet marked processed by `agent_slug`, oldest first.
    fn fetch_unprocessed(&self, agent_slug: &str, space: &str, limit: u32) -> Result<Vec<Item>>;

    /// Mark `(agent_slug, item)` processed. Idempotent; independent per agent.
    fn mark_processed(&self, agent_slug: &str, item_id: ItemId) -> Result<()>;

    /// Soft-delete an item: it disappears from `get_item`/`fetch_unprocessed`
    /// but its row and any prior `processed_marks` survive. Idempotent —
    /// deleting an already-deleted or missing item is a no-op.
    fn delete_item(&self, item_id: ItemId) -> Result<()>;
}

/// The text-bearing content of a message that the ingestion adapter decided is
/// worth storing. Unsupported payloads (binary without caption, etc.) are
/// handled inside the adapter — it replies in chat and never emits them here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingPayload {
    /// Plain text message. The daemon decides Text vs Link by inspecting it.
    Text(String),
    /// Caption attached to a non-text message (photo/video/document).
    Caption(String),
    /// Transcript of a voice message. The adapter has already downloaded the
    /// audio, run it through [`Transcription`], and deleted the temp file —
    /// the daemon only sees the resulting text.
    Voice(String),
}

/// A message pulled from an ingestion source, ready to be classified and stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMessage {
    pub chat_id: i64,
    pub message_id: i64,
    pub user_id: Option<i64>,
    pub username: Option<String>,
    pub date: i64,
    pub payload: IncomingPayload,
}

impl IncomingMessage {
    /// Apply ingestion classification rules to produce a [`NewItem`] bound to
    /// `source` and `space`: caption payloads stay captions; a plain-text
    /// payload that is itself a single URL becomes a [`ItemKind::Link`].
    pub fn into_new_item(self, source: String, space: Space) -> NewItem {
        let (kind, text) = match self.payload {
            IncomingPayload::Text(t) => {
                let kind = if is_url_only(&t) {
                    ItemKind::Link
                } else {
                    ItemKind::Text
                };
                (kind, t)
            }
            IncomingPayload::Caption(t) => (ItemKind::Caption, t),
            IncomingPayload::Voice(t) => (ItemKind::Voice, t),
        };
        NewItem {
            source,
            space,
            kind,
            text,
            telegram: TelegramMetadata {
                chat_id: self.chat_id,
                message_id: self.message_id,
                user_id: self.user_id,
                username: self.username,
                date: self.date,
            },
        }
    }
}

fn is_url_only(text: &str) -> bool {
    let trimmed = text.trim();
    (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
        && !trimmed.chars().any(char::is_whitespace)
}

/// Ingestion port — a stream of incoming messages from one source. The Telegram
/// adapter is the first implementation; webhook/import adapters can follow
/// without touching the daemon or domain.
#[async_trait]
pub trait IngestionSource: Send + Sync {
    fn slug(&self) -> &str;

    fn space(&self) -> &Space;

    /// Pull the next batch of messages. Implementations may long-poll: the call
    /// is allowed to block for tens of seconds before returning an empty batch.
    async fn poll(&mut self) -> Result<Vec<IncomingMessage>>;

    /// Confirm to the user that `message` was persisted as `item_id`. Interactive
    /// sources (Telegram) echo the saved text back with an inline "delete"
    /// affordance bound to `item_id`; non-interactive sources can leave the
    /// default no-op.
    async fn confirm_saved(&self, _message: &IncomingMessage, _item_id: ItemId) -> Result<()> {
        Ok(())
    }
}

/// Speech-to-text port. Implementations live behind a provider switch in
/// [`crate::adapters::transcription`]; the Telegram adapter calls this after
/// downloading a voice file and discards the bytes immediately after the call
/// returns (success or error).
///
/// `filename` is a hint for the provider: hosted APIs (OpenAI, Mistral) detect
/// the audio container from the extension during multipart upload, so passing
/// the original Telegram file name (e.g. `voice.oga`) matters.
#[async_trait]
pub trait Transcription: Send + Sync {
    async fn transcribe(&self, audio: &[u8], filename: &str) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload_text(s: &str) -> IncomingMessage {
        IncomingMessage {
            chat_id: 1,
            message_id: 2,
            user_id: None,
            username: None,
            date: 0,
            payload: IncomingPayload::Text(s.into()),
        }
    }

    #[test]
    fn caption_payload_classifies_as_caption() {
        let msg = IncomingMessage {
            payload: IncomingPayload::Caption("lunch".into()),
            ..payload_text("")
        };
        let item = msg.into_new_item("bot".into(), Space::new("inbox"));
        assert_eq!(item.kind, ItemKind::Caption);
        assert_eq!(item.text, "lunch");
    }

    #[test]
    fn url_only_text_classifies_as_link() {
        let item =
            payload_text("https://example.com/path").into_new_item("bot".into(), Space::new("x"));
        assert_eq!(item.kind, ItemKind::Link);
    }

    #[test]
    fn url_only_text_with_surrounding_whitespace_is_link() {
        let item =
            payload_text("   https://example.com\n").into_new_item("bot".into(), Space::new("x"));
        assert_eq!(item.kind, ItemKind::Link);
    }

    #[test]
    fn url_with_extra_words_is_text() {
        let item = payload_text("look https://example.com cool")
            .into_new_item("bot".into(), Space::new("x"));
        assert_eq!(item.kind, ItemKind::Text);
    }

    #[test]
    fn plain_text_classifies_as_text() {
        let item = payload_text("buy milk").into_new_item("bot".into(), Space::new("x"));
        assert_eq!(item.kind, ItemKind::Text);
    }

    #[test]
    fn voice_payload_classifies_as_voice() {
        let msg = IncomingMessage {
            payload: IncomingPayload::Voice("hello from audio".into()),
            ..payload_text("")
        };
        let item = msg.into_new_item("bot".into(), Space::new("inbox"));
        assert_eq!(item.kind, ItemKind::Voice);
        assert_eq!(item.text, "hello from audio");
    }
}
