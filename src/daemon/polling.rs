use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::domain::ports::Storage;

/// Ingestion polling loop. Empty scaffold for this slice: it ticks on the
/// configured interval but pulls nothing. Slice 2 wires Telegram long-polling
/// here via the [`IngestionSource`](crate::domain::ports::IngestionSource) port.
pub async fn run(config: Config, _storage: Arc<dyn Storage>) {
    let interval = Duration::from_secs(config.daemon.poll_interval_secs.max(1));
    tracing::info!(
        sources = config.sources.len(),
        interval_secs = config.daemon.poll_interval_secs,
        "polling loop started (scaffold)"
    );
    loop {
        tokio::time::sleep(interval).await;
        tracing::trace!("poll tick (no-op)");
    }
}
