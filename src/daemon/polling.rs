//! Ingestion polling loop. One task per configured source; each task owns its
//! [`IngestionSource`] adapter, long-polls it, and persists every message via
//! the [`Storage`] port.

use std::sync::Arc;
use std::time::Duration;

use crate::adapters::transcription;
use crate::adapters::TelegramSource;
use crate::config::{Config, SourceConfig};
use crate::domain::error::Result;
use crate::domain::ports::{IngestionSource, Storage};
use crate::domain::source::Space;

/// Cool-down after a poll error before retrying — keeps a flaky network or
/// rate-limited bot from spinning the CPU.
const BACKOFF_SECS: u64 = 5;

pub async fn run(config: Config, storage: Arc<dyn Storage>) {
    if config.sources.is_empty() {
        tracing::warn!("no sources configured; polling loop idle");
        std::future::pending::<()>().await;
        return;
    }
    tracing::info!(sources = config.sources.len(), "polling loop starting");
    let mut handles = Vec::new();
    for src in &config.sources {
        match build_source(src) {
            Ok(adapter) => {
                let storage = storage.clone();
                handles.push(tokio::spawn(run_source(adapter, storage)));
            }
            Err(e) => {
                tracing::error!(source = %src.slug, error = %e, "failed to build source");
            }
        }
    }
    for h in handles {
        let _ = h.await;
    }
}

fn build_source(src: &SourceConfig) -> Result<Box<dyn IngestionSource>> {
    let transcription = transcription::build(
        src.transcription_provider,
        src.transcription_token.as_deref(),
        src.transcription_url.as_deref(),
    )?;
    let adapter = TelegramSource::with_transcription(
        &src.slug,
        Space::new(src.space.clone()),
        &src.bot_token,
        transcription,
    )?;
    Ok(Box::new(adapter))
}

async fn run_source(mut source: Box<dyn IngestionSource>, storage: Arc<dyn Storage>) {
    let slug = source.slug().to_string();
    tracing::info!(source = %slug, space = %source.space(), "source poller started");
    loop {
        match source.poll().await {
            Ok(messages) => {
                if let Err(e) = persist_batch(source.as_ref(), &messages, storage.as_ref()) {
                    tracing::error!(source = %slug, error = %e, "persist batch failed");
                }
            }
            Err(e) => {
                tracing::warn!(source = %slug, error = %e, "poll failed; backing off");
                tokio::time::sleep(Duration::from_secs(BACKOFF_SECS)).await;
            }
        }
    }
}

fn persist_batch(
    source: &dyn IngestionSource,
    messages: &[crate::domain::ports::IncomingMessage],
    storage: &dyn Storage,
) -> Result<()> {
    for msg in messages {
        let item = msg
            .clone()
            .into_new_item(source.slug().to_string(), source.space().clone());
        let id = storage.insert_item(&item)?;
        tracing::debug!(
            source = source.slug(),
            chat_id = msg.chat_id,
            message_id = msg.message_id,
            item_id = %id,
            kind = item.kind.as_str(),
            "item stored"
        );
    }
    Ok(())
}
