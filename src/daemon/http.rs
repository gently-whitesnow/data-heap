use std::sync::Arc;

use crate::config::Config;
use crate::domain::ports::Storage;

/// HTTP API server. Empty scaffold for this slice: it logs the intended bind
/// address and idles. Slice 3 mounts the agent-facing endpoints (fetch new
/// items, mark processed) over the [`Storage`](Storage) port here.
pub async fn run(config: Config, _storage: Arc<dyn Storage>) {
    tracing::info!(addr = %config.daemon.http_addr, "HTTP API not yet served (scaffold)");
    std::future::pending::<()>().await;
}
