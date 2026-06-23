use std::sync::Arc;

use data_heap::adapters::SqliteStorage;
use data_heap::config::Config;
use data_heap::domain::ports::Storage;
use data_heap::{daemon, Result};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());
    tracing::info!(path = %config_path, "loading config");
    let config = Config::load(&config_path)?;

    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open(&config.daemon.database_path)?);
    tracing::info!(db = %config.daemon.database_path.display(), "storage ready");

    daemon::run(config, storage).await
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).init();
}
