//! Ingestion polling loop. One task per configured source; each task owns its
//! [`TelegramSource`] adapter, long-polls it, persists every message via the
//! [`Storage`] port, and obeys a shared [`CancellationToken`] for shutdown.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use tokio_util::sync::CancellationToken;

use crate::adapters::telegram::{PollOutcome, TelegramSource};
use crate::adapters::transcription;
use crate::config::SourceConfig;
use crate::domain::error::Result;
use crate::domain::ports::{IncomingMessage, IngestionSource, Storage};
use crate::domain::source::Space;

/// Backoff bounds: start small so transient hiccups don't pause ingestion
/// for long, cap so a persistent outage doesn't grow into a multi-minute
/// blackout that hides from the operator.
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

pub async fn run(
    sources: Vec<SourceConfig>,
    http: Client,
    storage: Arc<dyn Storage>,
    shutdown: CancellationToken,
) {
    if sources.is_empty() {
        tracing::warn!("no sources configured; polling loop idle");
        shutdown.cancelled().await;
        return;
    }
    tracing::info!(sources = sources.len(), "polling loop starting");
    let mut handles = Vec::new();
    for src in sources {
        match build_source(&src, &http, storage.clone()) {
            Ok(adapter) => {
                let storage = storage.clone();
                let shutdown = shutdown.clone();
                handles.push(tokio::spawn(run_source(adapter, storage, shutdown)));
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

fn build_source(
    src: &SourceConfig,
    http: &Client,
    storage: Arc<dyn Storage>,
) -> Result<TelegramSource> {
    let transcription = transcription::build(
        http,
        src.transcription_provider,
        src.transcription_token.as_ref(),
    )?;
    Ok(TelegramSource::with_transcription(
        src.slug.clone(),
        Space::new(src.space.clone()),
        clone_secret(&src.bot_token),
        transcription,
        http.clone(),
        storage,
        src.allowed_user_ids.iter().copied().collect(),
    ))
}

fn clone_secret(s: &secrecy::SecretString) -> secrecy::SecretString {
    use secrecy::ExposeSecret;
    secrecy::SecretString::from(s.expose_secret().to_owned())
}

async fn run_source(
    mut source: TelegramSource,
    storage: Arc<dyn Storage>,
    shutdown: CancellationToken,
) {
    let slug = source.slug().to_string();
    tracing::info!(source = %slug, space = %source.space(), "source poller started");
    let mut backoff = BACKOFF_MIN;
    loop {
        if shutdown.is_cancelled() {
            tracing::info!(source = %slug, "shutdown; poller exiting");
            return;
        }
        let outcome = tokio::select! {
            res = source.poll_outcome() => res,
            () = shutdown.cancelled() => {
                tracing::info!(source = %slug, "shutdown; poller exiting");
                return;
            }
        };
        match outcome {
            Ok(PollOutcome::Batch(messages)) => {
                backoff = BACKOFF_MIN;
                if let Err(e) = persist_batch(&source, messages, storage.as_ref()).await {
                    tracing::error!(source = %slug, error = %e, "persist batch failed");
                }
            }
            Ok(PollOutcome::RetryAfter(delay)) => {
                sleep_or_shutdown(delay, &shutdown).await;
            }
            Err(e) => {
                let delay = backoff_with_jitter(backoff);
                tracing::warn!(
                    source = %slug,
                    error = %e,
                    backoff_secs = delay.as_secs(),
                    "poll failed; backing off"
                );
                sleep_or_shutdown(delay, &shutdown).await;
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }
}

async fn sleep_or_shutdown(delay: Duration, shutdown: &CancellationToken) {
    tokio::select! {
        () = tokio::time::sleep(delay) => {}
        () = shutdown.cancelled() => {}
    }
}

/// Apply ±25% jitter so several pollers waking from a shared outage don't
/// thunder-herd the upstream when it recovers.
fn backoff_with_jitter(base: Duration) -> Duration {
    let base_ms = base.as_millis() as u64;
    if base_ms == 0 {
        return base;
    }
    let entropy = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::from(d.subsec_nanos()));
    let span = base_ms / 2; // ±25% means ±(base/4); span here is base/2 width
    let jitter = if span == 0 { 0 } else { entropy % span };
    let adjusted = base_ms.saturating_sub(span / 2).saturating_add(jitter);
    Duration::from_millis(adjusted)
}

async fn persist_batch(
    source: &TelegramSource,
    messages: Vec<IncomingMessage>,
    storage: &dyn Storage,
) -> Result<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let items: Vec<_> = messages
        .iter()
        .cloned()
        .map(|m| m.into_new_item(source.slug().to_string(), source.space().clone()))
        .collect();
    let ids = storage.insert_items(items.clone()).await?;
    for (msg, (item, id)) in messages.iter().zip(items.iter().zip(ids.iter())) {
        tracing::debug!(
            source = source.slug(),
            chat_id = msg.chat_id,
            message_id = msg.message_id,
            item_id = %id,
            kind = item.kind.as_str(),
            "item stored"
        );
        if let Err(e) = source.confirm_saved(msg, *id).await {
            tracing::warn!(
                source = source.slug(),
                item_id = %id,
                error = %e,
                "confirm_saved failed"
            );
        }
    }
    Ok(())
}
