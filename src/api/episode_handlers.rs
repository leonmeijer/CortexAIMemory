//! API handlers for Episodic Memory and Temporal Search

use super::handlers::{AppError, OrchestratorState};
use crate::indentiagraph::models::{CreateEpisodeRequest, EpisodeNode};
use crate::notes::Note;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ============================================================================
// Query Parameters
// ============================================================================

/// Query parameters for listing episodes
#[derive(Debug, Deserialize)]
pub struct EpisodesListQuery {
    /// Filter by project ID (UUID string)
    pub project_id: Option<String>,
    /// Filter by group ID (multi-tenancy)
    pub group_id: Option<String>,
    /// Maximum number of episodes to return (default: 20)
    pub limit: Option<u64>,
}

/// Query parameters for episode full-text search
#[derive(Debug, Deserialize)]
pub struct EpisodesSearchQuery {
    /// Search query string
    pub query: String,
    /// Filter by project ID (UUID string, optional)
    pub project_id: Option<String>,
    /// Maximum number of results (default: 20)
    pub limit: Option<u64>,
}

/// Query parameters for searching notes at a point in time
#[derive(Debug, Deserialize)]
pub struct NotesAtTimeQuery {
    /// Search query string
    pub query: String,
    /// ISO 8601 timestamp — return notes that were active at this time
    pub at_time: String,
    /// Filter by project ID (UUID string, optional)
    pub project_id: Option<String>,
    /// Maximum number of results (default: 10)
    pub limit: Option<u64>,
}

// ============================================================================
// Request Bodies
// ============================================================================

/// Request body for temporal note invalidation
#[derive(Debug, Deserialize)]
pub struct InvalidateTemporalBody {
    /// ISO 8601 timestamp at which the note becomes invalid.
    /// Defaults to `Utc::now()` if not provided.
    pub at_time: Option<String>,
}

// ============================================================================
// Response Types
// ============================================================================

/// Response for temporal invalidation
#[derive(Debug, Serialize)]
pub struct InvalidateTemporalResponse {
    pub success: bool,
    pub note_id: String,
    pub invalid_at: String,
}

// ============================================================================
// Handlers
// ============================================================================

/// The caller's ACL principal set (ADR-220) from the auth claims. Currently just
/// `user:<email>` (lowercased); extend with the Zitadel `principals` claim to add
/// group principals. Anonymous/no-email ⇒ empty ⇒ only public episodes are visible.
type ClaimsExt = Option<axum::Extension<crate::auth::jwt::Claims>>;

fn caller_principals(claims: &ClaimsExt) -> Vec<String> {
    match claims {
        Some(c) if !c.0.email.is_empty() => vec![cortex_core::principals::user(&c.0.email)],
        _ => Vec::new(),
    }
}

/// `POST /api/episodes` — Ingest a new episode into episodic memory
pub async fn add_episode(
    State(state): State<OrchestratorState>,
    claims: ClaimsExt,
    Json(mut body): Json<CreateEpisodeRequest>,
) -> Result<(StatusCode, Json<EpisodeNode>), AppError> {
    // Stamp owner from the authenticated caller when absent (provenance + the
    // default ACL subject). Un-ACL'd episodes stay public (empty allowed_principals)
    // for backward-compat; the queue/email path supplies allowed_principals.
    if body.owner.is_none() {
        if let Some(c) = claims.as_ref() {
            if !c.0.email.is_empty() {
                body.owner = Some(cortex_core::principals::user(&c.0.email));
            }
        }
    }
    let episode = state.orchestrator.indentiagraph().add_episode(body).await?;
    Ok((StatusCode::CREATED, Json(episode)))
}

/// `GET /api/episodes` — List recent episodes, optionally filtered by project or group
pub async fn list_episodes(
    State(state): State<OrchestratorState>,
    claims: ClaimsExt,
    Query(query): Query<EpisodesListQuery>,
) -> Result<Json<Vec<EpisodeNode>>, AppError> {
    let limit = query.limit.unwrap_or(20) as usize;
    let principals = caller_principals(&claims);

    let episodes = state
        .orchestrator
        .indentiagraph()
        .get_episodes(
            query.project_id.as_deref(),
            query.group_id.as_deref(),
            &principals,
            limit,
        )
        .await?;

    Ok(Json(episodes))
}

/// `GET /api/notes/at-time` — Search notes that were active at a specific point in time
pub async fn search_notes_at_time(
    State(state): State<OrchestratorState>,
    Query(query): Query<NotesAtTimeQuery>,
) -> Result<Json<Vec<Note>>, AppError> {
    let at: DateTime<Utc> = query.at_time.parse().map_err(|_| {
        AppError::BadRequest(format!(
            "Invalid at_time — expected ISO 8601 format, got: {}",
            query.at_time
        ))
    })?;

    let limit = query.limit.unwrap_or(10) as usize;

    let notes = state
        .orchestrator
        .indentiagraph()
        .search_notes_at_time(&query.query, at, query.project_id.as_deref(), limit)
        .await?;

    Ok(Json(notes))
}

/// `POST /api/notes/{note_id}/invalidate-temporal` — Mark a note as invalid at a specific time
pub async fn invalidate_note_temporal(
    State(state): State<OrchestratorState>,
    Path(note_id): Path<String>,
    Json(body): Json<InvalidateTemporalBody>,
) -> Result<Json<InvalidateTemporalResponse>, AppError> {
    let at: DateTime<Utc> = match body.at_time {
        Some(ref s) => s.parse().map_err(|_| {
            AppError::BadRequest(format!(
                "Invalid at_time — expected ISO 8601 format, got: {s}"
            ))
        })?,
        None => Utc::now(),
    };

    state
        .orchestrator
        .indentiagraph()
        .invalidate_note_at(&note_id, at)
        .await?;

    Ok(Json(InvalidateTemporalResponse {
        success: true,
        note_id: note_id.to_string(),
        invalid_at: at.to_rfc3339(),
    }))
}

/// `GET /api/episodes/search` — Search episodes by content using BM25 full-text search
pub async fn search_episodes(
    State(state): State<OrchestratorState>,
    claims: ClaimsExt,
    Query(query): Query<EpisodesSearchQuery>,
) -> Result<Json<Vec<EpisodeNode>>, AppError> {
    let limit = query.limit.unwrap_or(20) as usize;
    let principals = caller_principals(&claims);

    let episodes = state
        .orchestrator
        .indentiagraph()
        .search_episodes(&query.query, query.project_id.as_deref(), &principals, limit)
        .await?;

    Ok(Json(episodes))
}
