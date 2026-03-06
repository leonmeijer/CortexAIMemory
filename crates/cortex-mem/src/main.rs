//! cortex-mem worker daemon.
//!
//! Starts an HTTP server that hooks communicate with to store and retrieve
//! memory data from the IndentiaGraph.

use anyhow::Result;
use cortex_indentiagraph::IndentiaGraphStore;
use cortex_mem::config::MemConfig;
use cortex_mem::server::{start_worker, WorkerState};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present
    let _ = dotenvy::dotenv();

    // Initialize logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("cortex_mem=info".parse()?))
        .init();

    // Load config
    let config = MemConfig::load();
    info!("cortex-mem worker starting...");
    info!("  SurrealDB: {}", config.surrealdb_url);
    info!("  Port: {}", config.worker_port);

    // Connect to IndentiaGraph
    let store = IndentiaGraphStore::new(
        &config.surrealdb_url,
        &config.surrealdb_namespace,
        &config.surrealdb_database,
        &config.surrealdb_username,
        &config.surrealdb_password,
    )
    .await?;

    store.init_schema().await?;
    info!("IndentiaGraph schema initialized");

    let state = WorkerState {
        store: Arc::new(store),
        config: Arc::new(config),
    };

    start_worker(state).await
}
