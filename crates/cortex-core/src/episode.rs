//! Episodic memory types — Graphiti-inspired temporal knowledge capture

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Source type for an episode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeSource {
    /// From a conversation or chat session
    Conversation,
    /// From a code commit or code review
    Code,
    /// From an imported document
    Document,
    /// From a system event (file change, build, test, etc.)
    Event,
}

impl std::fmt::Display for EpisodeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EpisodeSource::Conversation => write!(f, "conversation"),
            EpisodeSource::Code => write!(f, "code"),
            EpisodeSource::Document => write!(f, "document"),
            EpisodeSource::Event => write!(f, "event"),
        }
    }
}

impl std::str::FromStr for EpisodeSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "conversation" => Ok(EpisodeSource::Conversation),
            "code" => Ok(EpisodeSource::Code),
            "document" => Ok(EpisodeSource::Document),
            "event" => Ok(EpisodeSource::Event),
            other => Err(format!("Unknown episode source: {other}")),
        }
    }
}

/// A timestamped episode — raw text ingested into episodic memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    /// Unique identifier (SurrealDB record ID string, e.g. "episode:uuid")
    pub id: String,
    /// Human-readable name/title
    pub name: String,
    /// Raw text content
    pub content: String,
    /// Source type
    pub source: EpisodeSource,
    /// When the episode's event occurred (user-supplied or defaults to ingestion time)
    pub reference_time: DateTime<Utc>,
    /// When the episode was ingested into the system
    pub ingested_at: DateTime<Utc>,
    /// Optional project scope
    pub project_id: Option<String>,
    /// Optional group for multi-tenancy isolation
    pub group_id: Option<String>,
}

/// Request to create a new episode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEpisodeRequest {
    /// Human-readable name/title
    pub name: String,
    /// Raw text content
    pub content: String,
    /// Source type
    pub source: EpisodeSource,
    /// When the episode's event occurred (defaults to now if None)
    pub reference_time: Option<DateTime<Utc>>,
    /// Optional project scope
    pub project_id: Option<String>,
    /// Optional group for multi-tenancy isolation
    pub group_id: Option<String>,
}
