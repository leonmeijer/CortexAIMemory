//! Legacy `indentiagraph` compatibility namespace.
//!
//! Runtime storage is provided by IndentiaGraph (SurrealDB) via `cortex-graph`.

pub mod models;
pub mod traits;

pub use models::*;
pub use traits::GraphStore;

#[cfg(test)]
pub(crate) mod batch;
#[cfg(test)]
pub mod mock;
