//! Worker daemon HTTP server.
//!
//! Runs on port 37777 (configurable) and provides the API that hooks call.

use anyhow::Result;
use axum::{
    extract::{Json, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use cortex_core::notes::{Note, NoteFilters, NoteImportance, NoteScope, NoteStatus, NoteType};
use cortex_graph::traits::GraphStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::config::MemConfig;
use crate::context;
use crate::observation::{self, RawObservation};
use crate::session;

/// Shared worker state.
#[derive(Clone)]
pub struct WorkerState {
    pub store: Arc<dyn GraphStore>,
    pub config: Arc<MemConfig>,
}

/// Start the worker HTTP server.
pub async fn start_worker(state: WorkerState) -> Result<()> {
    let addr = format!("{}:{}", state.config.worker_host, state.config.worker_port);
    info!("cortex-mem worker starting on {}", addr);

    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Create the axum router with all endpoints.
pub fn create_router(state: WorkerState) -> Router {
    Router::new()
        // Health
        .route("/health", get(health))
        // Session management
        .route("/api/sessions/init", post(session_init))
        .route("/api/sessions/observations", post(session_observation))
        .route("/api/sessions/summarize", post(session_summarize))
        .route("/api/sessions/complete", post(session_complete))
        // Context injection
        .route("/api/context/inject", get(context_inject))
        // Search
        .route("/api/search", get(search))
        // Admin
        .route("/api/admin/shutdown", post(admin_shutdown))
        .with_state(state)
}

// ============================================================================
// Handlers
// ============================================================================

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "service": "cortex-mem" }))
}

// --- Session Init ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionInitRequest {
    content_session_id: String,
    cwd: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionInitResponse {
    session_id: String,
    project_slug: Option<String>,
    is_new: bool,
}

async fn session_init(
    State(state): State<WorkerState>,
    Json(req): Json<SessionInitRequest>,
) -> Result<Json<SessionInitResponse>, StatusCode> {
    let project_slug = session::resolve_project_slug(&state.store, &req.cwd).await;

    let sess = session::init_session(
        &state.store,
        &req.content_session_id,
        &req.cwd,
        project_slug.as_deref(),
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to init session: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(SessionInitResponse {
        session_id: sess.id.to_string(),
        project_slug,
        is_new: true,
    }))
}

// --- Observation ---

async fn session_observation(
    State(state): State<WorkerState>,
    Json(obs): Json<RawObservation>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Resolve project
    let project_slug = session::resolve_project_slug(&state.store, &obs.cwd).await;
    let project_id = if let Some(ref slug) = project_slug {
        state
            .store
            .get_project_by_slug(slug)
            .await
            .ok()
            .flatten()
            .map(|p| p.id)
    } else {
        None
    };

    match observation::process_observation(&state.store, &obs, project_id, &state.config.skip_tools)
        .await
    {
        Ok(Some(processed)) => Ok(Json(serde_json::json!({
            "status": "stored",
            "note_id": processed.note_id.to_string(),
            "type": processed.observation_type,
            "title": processed.title,
        }))),
        Ok(None) => Ok(Json(serde_json::json!({
            "status": "skipped",
            "reason": "filtered or duplicate",
        }))),
        Err(e) => {
            tracing::error!("Failed to process observation: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// --- Summarize ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummarizeRequest {
    content_session_id: String,
}

async fn session_summarize(
    State(state): State<WorkerState>,
    Json(req): Json<SummarizeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Find the session's project
    let project_slug = match state.store.list_chat_sessions(None, None, 100, 0).await {
        Ok((sessions, _)) => sessions
            .iter()
            .find(|s| s.cli_session_id.as_deref() == Some(&req.content_session_id))
            .and_then(|s| s.project_slug.clone()),
        Err(_) => None,
    };

    let project_id = if let Some(ref slug) = project_slug {
        state
            .store
            .get_project_by_slug(slug)
            .await
            .ok()
            .flatten()
            .map(|p| p.id)
    } else {
        None
    };

    // Create a summary note placeholder
    if let Some(pid) = project_id {
        let note = Note::new_full(
            Some(pid),
            NoteType::Context,
            NoteImportance::Medium,
            NoteScope::Project,
            format!(
                "Session summary for {} (auto-generated)",
                req.content_session_id
            ),
            vec![],
            "cortex-mem".to_string(),
        );
        let _ = state.store.create_note(&note).await;
    }

    Ok(Json(serde_json::json!({ "status": "summarized" })))
}

// --- Complete ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompleteRequest {
    content_session_id: String,
}

async fn session_complete(
    State(state): State<WorkerState>,
    Json(req): Json<CompleteRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if let Ok((sessions, _)) = state.store.list_chat_sessions(None, None, 100, 0).await {
        if let Some(s) = sessions
            .iter()
            .find(|s| s.cli_session_id.as_deref() == Some(&req.content_session_id))
        {
            let _ = session::complete_session(&state.store, s.id).await;
        }
    }

    Ok(Json(serde_json::json!({ "status": "completed" })))
}

// --- Context Injection ---

#[derive(Deserialize)]
struct ContextQuery {
    #[serde(default)]
    projects: Option<String>,
}

async fn context_inject(
    State(state): State<WorkerState>,
    Query(query): Query<ContextQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let project_slug = query.projects.as_deref().and_then(|p| p.split(',').next());

    let ctx = context::generate_context(
        &state.store,
        project_slug,
        state.config.context_observations,
        state.config.context_show_last_summary,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to generate context: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(serde_json::json!({
        "context": ctx,
        "has_content": !ctx.is_empty(),
    })))
}

// --- Search ---

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    query: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

async fn search(
    State(state): State<WorkerState>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let query_text = params.query.unwrap_or_default();
    if query_text.is_empty() {
        return Ok(Json(serde_json::json!({ "results": [] })));
    }

    // Search notes using NoteFilters with search field
    let filters = NoteFilters {
        search: Some(query_text),
        status: Some(vec![NoteStatus::Active]),
        limit: Some(params.limit as i64),
        ..Default::default()
    };

    match state.store.list_notes(None, None, &filters).await {
        Ok((notes, _total)) => {
            let results: Vec<serde_json::Value> = notes
                .iter()
                .map(|n| {
                    serde_json::json!({
                        "id": n.id.to_string(),
                        "type": format!("{:?}", n.note_type),
                        "content": n.content,
                        "created_at": n.created_at.to_rfc3339(),
                    })
                })
                .collect();
            Ok(Json(serde_json::json!({ "results": results })))
        }
        Err(e) => {
            tracing::error!("Search failed: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// --- Admin ---

async fn admin_shutdown() -> impl IntoResponse {
    info!("Shutdown requested");
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        std::process::exit(0);
    });
    Json(serde_json::json!({ "status": "shutting_down" }))
}
