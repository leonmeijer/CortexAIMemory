//! CortexAIMemory Core — domain types, models, enums, and errors.
//!
//! This crate contains all backend-agnostic data types shared across
//! the CortexAIMemory workspace. No database dependencies.

pub mod episode;
pub mod events;
pub mod graph;
pub mod models;
pub mod neurons;
pub mod notes;
pub mod parser_types;
pub mod plan;
pub mod skills;
pub mod test_helpers;

pub use episode::{CreateEpisodeRequest, Episode, EpisodeSource};
