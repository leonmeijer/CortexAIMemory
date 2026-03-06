//! IndentiaGraph (SurrealDB) backend for CortexAIMemory.
//!
//! This crate implements the [`GraphStore`] trait from `cortex-graph` using
//! SurrealDB as the storage engine. It provides:
//!
//! - Connection management (remote WebSocket, embedded memory for tests)
//! - Schema initialization (tables, fields, indexes, vector indexes)
//! - Full GraphStore implementation (368+ methods)
//!
//! # Connection Modes
//!
//! - **Remote** (`ws://host:port`): Connect to a running SurrealDB instance
//! - **Memory** (`mem://`): In-memory database for tests (no external service)

pub mod client;
pub mod schema;

// Domain-specific modules (each implements a slice of GraphStore)
mod analytics;
mod chat;
mod code;
mod commit;
mod constraint;
mod decision;
mod episode;
mod fabric;
mod feature_graph;
mod file;
mod milestone;
mod note;
mod plan;
mod project;
mod release;
mod skill;
mod step;
mod symbol;
pub(crate) mod task;
mod user;
mod workspace;

// GraphStore trait implementation (delegates to domain modules)
mod impl_graph_store;

// Re-exports
pub use client::IndentiaGraphStore;
