//! Daemon scaffold: wires storage to the polling and HTTP loops and runs them
//! until shutdown. Both loops are empty stubs in this slice.

mod http;
mod polling;

use std::sync::Arc;

use crate::config::Config;
use crate::domain::error::{Error, Result};
use crate::domain::ports::Storage;

/// Sync config-declared sources into storage, then run the polling and HTTP
/// loops concurrently until Ctrl-C.
pub async fn run(config: Config, storage: Arc<dyn Storage>) -> Result<()> {
    sync_sources(&config, storage.as_ref())?;

    let poll = tokio::spawn(polling::run(config.clone(), storage.clone()));
    let http = tokio::spawn(http::run(config.clone(), storage.clone()));

    tracing::info!("data-heap daemon running; press Ctrl-C to stop");
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| Error::Io(std::io::Error::other(e)))?;

    tracing::info!("shutdown signal received, stopping daemon");
    poll.abort();
    http.abort();
    Ok(())
}

/// Reconcile the source registry with config: every configured source is
/// upserted so the daemon's view matches the TOML on each start.
fn sync_sources(config: &Config, storage: &dyn Storage) -> Result<()> {
    for source in &config.sources {
        storage.upsert_source(&source.to_source())?;
    }
    tracing::info!(count = config.sources.len(), "sources synced into storage");
    Ok(())
}
