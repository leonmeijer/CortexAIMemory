//! CortexAIMemory Graph — GraphStore trait, analytics engine, embeddings, and mock backend.
//!
//! This crate defines the backend-agnostic `GraphStore` trait (368+ methods)
//! and provides the analytics engine (PageRank, betweenness, communities),
//! embedding providers, and a mock implementation for testing.

pub mod embeddings;
pub mod mock;
pub mod traits;

pub use traits::GraphStore;

/// A single result from a BM25 code search across functions and structs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeSearchHit {
    /// File path containing the matched symbol
    pub path: String,
    /// Programming language of the file
    pub language: String,
    /// Symbol names matched (function/struct names)
    pub symbols: Vec<String>,
    /// Best matching docstring from the symbols
    pub docstring: String,
    /// BM25 relevance score
    pub score: f64,
    /// Project ID (if available)
    pub project_id: Option<String>,
    /// Project slug (if available)
    pub project_slug: Option<String>,
}
