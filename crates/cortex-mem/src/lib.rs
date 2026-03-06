//! cortex-mem — Claude Code memory plugin backed by IndentiaGraph.
//!
//! Replaces the Node.js claude-mem plugin with pure Rust binaries:
//! - `cortex-mem`: Worker daemon (axum HTTP, port 37777)
//! - `cortex-mem-hook`: Hook binary (reads stdin, dispatches to worker)

pub mod config;
pub mod context;
pub mod hooks;
pub mod observation;
pub mod server;
pub mod session;

// Re-exports
pub use config::MemConfig;
