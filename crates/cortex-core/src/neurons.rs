//! Neural activation types used in spreading activation and skill context.

use crate::notes::Note;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How a note was activated during spreading activation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActivationSource {
    /// Activated by direct vector similarity match.
    Direct,
    /// Activated by spreading through synapses from another note.
    Propagated {
        /// ID of the note that propagated the activation.
        via: Uuid,
        /// Number of hops from the nearest direct-match ancestor.
        hops: usize,
    },
}

fn default_entity_type() -> String {
    "note".to_string()
}

/// A note activated during spreading activation retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivatedNote {
    /// The note itself (for Decision entities, a synthetic Note is created).
    pub note: Note,
    /// Final activation score (0.0 - 1.0+).
    pub activation_score: f64,
    /// How this note was activated.
    pub source: ActivationSource,
    /// Entity type: "note" for knowledge notes, "decision" for architectural decisions.
    #[serde(default = "default_entity_type")]
    pub entity_type: String,
}
