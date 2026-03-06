//! Memory session handlers for cortex-mem hook integration.
//!
//! These endpoints are called by the `cortex-mem-hook` binary during Claude Code
//! hook events (SessionStart, PostToolUse, Stop). They store observations as Notes
//! and generate context for injection.

use super::handlers::OrchestratorState;
use axum::{
    extract::{Json, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::Utc;
use cortex_core::notes::{EntityType, Note, NoteFilters, NoteImportance, NoteScope, NoteStatus, NoteType};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Session Init
// ============================================================================

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInitRequest {
    content_session_id: String,
    cwd: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInitResponse {
    session_id: String,
    project_slug: Option<String>,
    is_new: bool,
}

pub async fn session_init(
    State(state): State<OrchestratorState>,
    Json(req): Json<SessionInitRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let ig = state.orchestrator.indentiagraph();

    // Resolve project from cwd
    let project_slug = resolve_project_slug(ig, &req.cwd).await;

    // Check if session already exists
    if let Ok((sessions, _)) = ig
        .list_chat_sessions(project_slug.as_deref(), None, 100, 0)
        .await
    {
        for session in &sessions {
            if session.cli_session_id.as_deref() == Some(&req.content_session_id) {
                return Ok(Json(SessionInitResponse {
                    session_id: session.id.to_string(),
                    project_slug,
                    is_new: false,
                }));
            }
        }
    }

    // Create new session
    let session = cortex_core::models::ChatSessionNode {
        id: Uuid::new_v4(),
        cli_session_id: Some(req.content_session_id),
        project_slug: project_slug.clone(),
        workspace_slug: None,
        cwd: req.cwd,
        title: None,
        model: "cortex-mem".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        message_count: 0,
        total_cost_usd: None,
        conversation_id: None,
        preview: None,
        permission_mode: None,
        add_dirs: None,
    };

    ig.create_chat_session(&session).await.map_err(|e| {
        tracing::error!("Failed to create chat session: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(SessionInitResponse {
        session_id: session.id.to_string(),
        project_slug,
        is_new: true,
    }))
}

// ============================================================================
// Observation (PostToolUse)
// ============================================================================

/// Tools to skip when capturing observations.
const SKIP_TOOLS: &[&str] = &[
    "ListMcpResourcesTool",
    "SlashCommand",
    "Skill",
    "TodoWrite",
    "AskUserQuestion",
    "TaskCreate",
    "TaskUpdate",
    "TaskGet",
    "TaskList",
];

#[derive(Debug, Deserialize)]
pub struct RawObservation {
    pub content_session_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_response: serde_json::Value,
    pub cwd: String,
}

pub async fn session_observation(
    State(state): State<OrchestratorState>,
    Json(obs): Json<RawObservation>,
) -> Result<impl IntoResponse, StatusCode> {
    // Skip filtered tools
    if SKIP_TOOLS.iter().any(|t| *t == obs.tool_name) {
        return Ok(Json(serde_json::json!({
            "status": "skipped",
            "reason": "filtered tool",
        })));
    }

    let ig = state.orchestrator.indentiagraph();

    // Resolve project
    let project_slug = resolve_project_slug(ig, &obs.cwd).await;
    let project_id = if let Some(ref slug) = project_slug {
        ig.get_project_by_slug(slug)
            .await
            .ok()
            .flatten()
            .map(|p| p.id)
    } else {
        None
    };

    // Build content
    let content = build_observation_content(&obs);
    if content.is_empty() {
        return Ok(Json(serde_json::json!({
            "status": "skipped",
            "reason": "empty content",
        })));
    }

    let title = build_title(&obs.tool_name, &obs.tool_input);
    let source_files = extract_source_files(&obs.tool_input);

    // Create Note
    let note = Note::new_full(
        project_id,
        NoteType::Observation,
        NoteImportance::Medium,
        NoteScope::Project,
        format!("## {}\n\n{}", title, content),
        source_files.iter().map(|s| s.to_string()).collect(),
        "cortex-mem".to_string(),
    );

    ig.create_note(&note).await.map_err(|e| {
        tracing::error!("Failed to store observation note: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Link to project
    if let Some(pid) = project_id {
        let _ = ig
            .link_note_to_entity(note.id, &EntityType::Project, &pid.to_string(), None, None)
            .await;
    }

    // Link to source files
    for file_path in &source_files {
        let _ = ig
            .link_note_to_entity(note.id, &EntityType::File, file_path, None, None)
            .await;
    }

    Ok(Json(serde_json::json!({
        "status": "stored",
        "note_id": note.id.to_string(),
        "title": title,
    })))
}

// ============================================================================
// Summarize (Stop phase 1)
// ============================================================================

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummarizeRequest {
    content_session_id: String,
}

pub async fn session_summarize(
    State(state): State<OrchestratorState>,
    Json(req): Json<SummarizeRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let ig = state.orchestrator.indentiagraph();

    // Find project for this session
    let project_id = find_session_project(ig, &req.content_session_id).await;

    // Create summary note
    if let Some(pid) = project_id {
        let note = Note::new_full(
            Some(pid),
            NoteType::Context,
            NoteImportance::Medium,
            NoteScope::Project,
            format!(
                "Session summary for {} (auto-generated at {})",
                req.content_session_id,
                Utc::now().format("%Y-%m-%d %H:%M")
            ),
            vec![],
            "cortex-mem".to_string(),
        );
        let _ = ig.create_note(&note).await;
    }

    Ok(Json(serde_json::json!({ "status": "summarized" })))
}

// ============================================================================
// Complete (Stop phase 2)
// ============================================================================

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteRequest {
    content_session_id: String,
}

pub async fn session_complete(
    State(state): State<OrchestratorState>,
    Json(req): Json<CompleteRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let ig = state.orchestrator.indentiagraph();

    if let Ok((sessions, _)) = ig.list_chat_sessions(None, None, 100, 0).await {
        if let Some(s) = sessions
            .iter()
            .find(|s| s.cli_session_id.as_deref() == Some(&req.content_session_id))
        {
            let _ = ig
                .update_chat_session(s.id, None, None, None, None, None, None)
                .await;
        }
    }

    Ok(Json(serde_json::json!({ "status": "completed" })))
}

// ============================================================================
// Context Injection (SessionStart)
// ============================================================================

#[derive(Deserialize)]
pub struct ContextQuery {
    #[serde(default)]
    projects: Option<String>,
}

pub async fn context_inject(
    State(state): State<OrchestratorState>,
    Query(query): Query<ContextQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let ig = state.orchestrator.indentiagraph();
    let project_slug = query.projects.as_deref().and_then(|p| p.split(',').next());

    let ctx = generate_context(ig, project_slug, 50, true).await.map_err(|e| {
        tracing::error!("Failed to generate context: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(serde_json::json!({
        "context": ctx,
        "has_content": !ctx.is_empty(),
    })))
}

// ============================================================================
// Helpers
// ============================================================================

async fn resolve_project_slug(
    store: &dyn crate::indentiagraph::GraphStore,
    cwd: &str,
) -> Option<String> {
    let projects = store.list_projects().await.ok()?;
    for project in &projects {
        let root = if project.root_path.ends_with('/') {
            project.root_path.clone()
        } else {
            format!("{}/", project.root_path)
        };
        if cwd.starts_with(&root) || cwd == project.root_path {
            return Some(project.slug.clone());
        }
    }
    None
}

async fn find_session_project(
    store: &dyn crate::indentiagraph::GraphStore,
    content_session_id: &str,
) -> Option<Uuid> {
    let (sessions, _) = store.list_chat_sessions(None, None, 100, 0).await.ok()?;
    let session = sessions
        .iter()
        .find(|s| s.cli_session_id.as_deref() == Some(content_session_id))?;
    let slug = session.project_slug.as_deref()?;
    store
        .get_project_by_slug(slug)
        .await
        .ok()
        .flatten()
        .map(|p| p.id)
}

async fn generate_context(
    store: &dyn crate::indentiagraph::GraphStore,
    project_slug: Option<&str>,
    max_observations: usize,
    show_last_summary: bool,
) -> anyhow::Result<String> {
    let mut sections = Vec::new();

    let project_id = if let Some(slug) = project_slug {
        store.get_project_by_slug(slug).await.ok().flatten().map(|p| p.id)
    } else {
        None
    };

    // 1. Recent observations
    if let Some(pid) = project_id {
        let filters = NoteFilters {
            note_type: Some(vec![NoteType::Observation]),
            status: Some(vec![NoteStatus::Active]),
            limit: Some(max_observations as i64),
            ..Default::default()
        };
        if let Ok((notes, _)) = store.list_notes(Some(pid), None, &filters).await {
            if !notes.is_empty() {
                let mut s = String::from("## Recent Observations\n\n");
                for (i, note) in notes.iter().enumerate().take(max_observations) {
                    let age = Utc::now().signed_duration_since(note.created_at);
                    let age_str = if age.num_hours() < 1 {
                        format!("{}m ago", age.num_minutes())
                    } else if age.num_hours() < 24 {
                        format!("{}h ago", age.num_hours())
                    } else {
                        format!("{}d ago", age.num_days())
                    };
                    let title = note
                        .content
                        .lines()
                        .next()
                        .unwrap_or("Observation")
                        .trim_start_matches('#')
                        .trim();
                    s.push_str(&format!("{}. **{}** ({})\n", i + 1, title, age_str));
                }
                sections.push(s);
            }
        }
    }

    // 2. Last session summary
    if show_last_summary {
        if let Some(pid) = project_id {
            let filters = NoteFilters {
                note_type: Some(vec![NoteType::Context]),
                status: Some(vec![NoteStatus::Active]),
                limit: Some(1),
                ..Default::default()
            };
            if let Ok((notes, _)) = store.list_notes(Some(pid), None, &filters).await {
                if let Some(note) = notes.first() {
                    let mut s = String::from("## Last Session Summary\n\n");
                    s.push_str(&note.content);
                    s.push('\n');
                    sections.push(s);
                }
            }
        }
    }

    // 3. Knowledge notes (guidelines, gotchas, patterns)
    if let Some(pid) = project_id {
        let important_types = [NoteType::Guideline, NoteType::Gotcha, NoteType::Pattern];
        let mut knowledge_notes = Vec::new();
        for note_type in &important_types {
            let filters = NoteFilters {
                note_type: Some(vec![*note_type]),
                status: Some(vec![NoteStatus::Active]),
                limit: Some(5),
                ..Default::default()
            };
            if let Ok((notes, _)) = store.list_notes(Some(pid), None, &filters).await {
                knowledge_notes.extend(notes);
            }
        }
        if !knowledge_notes.is_empty() {
            let mut s = String::from("## Knowledge Notes\n\n");
            for note in &knowledge_notes {
                let type_label = format!("{:?}", note.note_type).to_lowercase();
                let first_line = note
                    .content
                    .lines()
                    .next()
                    .unwrap_or("Note")
                    .trim_start_matches('#')
                    .trim();
                s.push_str(&format!("- **[{}]** {}\n", type_label, first_line));
            }
            sections.push(s);
        }
    }

    if sections.is_empty() {
        return Ok(String::new());
    }

    let mut output = String::from("# Memory Context (cortex-mem)\n\n");
    output.push_str(&sections.join("\n"));
    Ok(output)
}

fn build_observation_content(obs: &RawObservation) -> String {
    let input_str = if obs.tool_input.is_string() {
        obs.tool_input.as_str().unwrap_or("").to_string()
    } else {
        serde_json::to_string_pretty(&obs.tool_input).unwrap_or_default()
    };

    let response_str = if obs.tool_response.is_string() {
        obs.tool_response.as_str().unwrap_or("").to_string()
    } else {
        serde_json::to_string_pretty(&obs.tool_response).unwrap_or_default()
    };

    // Truncate very long responses
    let max_len = 4000;
    let response_truncated = if response_str.len() > max_len {
        format!(
            "{}...\n[truncated {} chars]",
            &response_str[..max_len],
            response_str.len() - max_len
        )
    } else {
        response_str
    };

    format!(
        "**Tool**: {}\n**Input**: {}\n**Response**: {}",
        obs.tool_name, input_str, response_truncated
    )
}

fn build_title(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "Edit" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("unknown");
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(path);
            format!("Edited {}", filename)
        }
        "Write" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("unknown");
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(path);
            format!("Created {}", filename)
        }
        "Read" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("unknown");
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(path);
            format!("Read {}", filename)
        }
        "Bash" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("command");
            let short = if cmd.len() > 60 { &cmd[..60] } else { cmd };
            format!("Ran: {}", short)
        }
        "Grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("pattern");
            format!("Searched for '{}'", pattern)
        }
        "Glob" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("pattern");
            format!("Found files matching '{}'", pattern)
        }
        _ => format!("Used {}", tool_name),
    }
}

fn extract_source_files(input: &serde_json::Value) -> Vec<String> {
    let mut files = Vec::new();
    if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
        files.push(path.to_string());
    }
    if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
        if !files.contains(&path.to_string()) {
            files.push(path.to_string());
        }
    }
    files
}
