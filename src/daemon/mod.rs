//! Daemon entry point: wires storage to the ingestion polling loop and the
//! consumer-facing HTTP API, then runs both concurrently until shutdown.

mod http;
mod polling;

use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use tokio_util::sync::CancellationToken;

use crate::config::{Config, SourceConfig};
use crate::domain::error::{Error, Result};
use crate::domain::ports::Storage;

/// HTTP client timeout. Covers Telegram long-poll headroom (25s) and slow
/// hosted transcription providers (60s cold start); per-call sleeps are not
/// added on top.
const HTTP_TIMEOUT_SECS: u64 = 90;

/// Sync config-declared sources into storage, then run the polling and HTTP
/// loops concurrently until Ctrl-C.
pub async fn run(config: Config, storage: Arc<dyn Storage>) -> Result<()> {
    sync_sources(&config.sources, storage.as_ref()).await?;

    let http_client = build_http_client()?;
    let shutdown = CancellationToken::new();

    let polling_handle = tokio::spawn(polling::run(
        config.sources,
        http_client.clone(),
        storage.clone(),
        shutdown.clone(),
    ));
    let http_handle = tokio::spawn(http::run(
        config.daemon.http_addr,
        storage.clone(),
        shutdown.clone(),
    ));

    tracing::info!("data-heap daemon running; press Ctrl-C to stop");
    tokio::select! {
        res = tokio::signal::ctrl_c() => {
            res.map_err(|e| Error::Io(std::io::Error::other(e)))?;
            tracing::info!("shutdown signal received, stopping daemon");
        }
        () = shutdown.cancelled() => {
            tracing::info!("shutdown signalled internally");
        }
    }
    shutdown.cancel();

    let _ = tokio::join!(polling_handle, http_handle);
    Ok(())
}

fn build_http_client() -> Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()?)
}

async fn sync_sources(sources: &[SourceConfig], storage: &dyn Storage) -> Result<()> {
    for source in sources {
        storage.upsert_source(source.to_source()).await?;
    }
    tracing::info!(count = sources.len(), "sources synced into storage");
    Ok(())
}
