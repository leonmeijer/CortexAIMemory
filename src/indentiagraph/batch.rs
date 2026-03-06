//! Test-only batch helpers retained for legacy stress tests.

use std::collections::HashMap;

/// Chunk size used by legacy UNWIND batching tests.
pub const BATCH_SIZE: usize = 10_000;

/// Lightweight map payload used in stress tests.
pub type BoltMap = HashMap<String, serde_json::Value>;

/// Build a [`BoltMap`] from key/value pairs.
#[allow(dead_code)]
pub fn bolt_map(pairs: &[(&str, serde_json::Value)]) -> BoltMap {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}
