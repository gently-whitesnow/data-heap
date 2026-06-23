use super::error::Result;
use super::item::{Item, ItemId, NewItem};
use super::source::Source;

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
}

/// A message pulled from an ingestion source, before domain ingestion rules
/// decide what (if anything) to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMessage {
    pub text: Option<String>,
    pub chat_id: i64,
    pub message_id: i64,
    pub user_id: Option<i64>,
    pub username: Option<String>,
    pub date: i64,
    /// File id of an attached voice message, if any (downloaded later for STT).
    pub voice_file_id: Option<String>,
}

/// Ingestion port — a stream of incoming messages from one source (Telegram
/// long-polling first). No adapter in this slice; the daemon scaffold is empty.
pub trait IngestionSource: Send {
    fn slug(&self) -> &str;

    /// Pull the next batch of messages. Returns an empty vec when idle.
    fn poll(&mut self) -> Result<Vec<IncomingMessage>>;
}

/// Speech-to-text port. No adapter in this slice.
pub trait Transcription: Send + Sync {
    fn transcribe(&self, audio: &[u8]) -> Result<String>;
}
