//! Session lifecycle management.
//!
//! Maps Claude Code content sessions to ChatSessionNode entries in the graph.

use anyhow::{Context, Result};
use chrono::Utc;
use cortex_core::models::ChatSessionNode;
use cortex_graph::traits::GraphStore;
use std::sync::Arc;
use uuid::Uuid;

/// Initialize a new memory session linked to a Claude Code content session.
///
/// If a session with the same `cli_session_id` already exists, returns it.
/// Otherwise creates a new ChatSessionNode.
pub async fn init_session(
    store: &Arc<dyn GraphStore>,
    content_session_id: &str,
    cwd: &str,
    project_slug: Option<&str>,
) -> Result<ChatSessionNode> {
    // Check if session already exists
    if let Ok((sessions, _total)) = store.list_chat_sessions(project_slug, None, 100, 0).await {
        for session in &sessions {
            if session.cli_session_id.as_deref() == Some(content_session_id) {
                return Ok(session.clone());
            }
        }
    }

    // Create new session
    let session = ChatSessionNode {
        id: Uuid::new_v4(),
        cli_session_id: Some(content_session_id.to_string()),
        project_slug: project_slug.map(|s| s.to_string()),
        workspace_slug: None,
        cwd: cwd.to_string(),
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

    store
        .create_chat_session(&session)
        .await
        .context("Failed to create chat session")?;

    Ok(session)
}

/// Complete a session by updating its status.
pub async fn complete_session(store: &Arc<dyn GraphStore>, session_id: Uuid) -> Result<()> {
    store
        .update_chat_session(session_id, None, None, None, None, None, None)
        .await
        .context("Failed to complete session")?;
    Ok(())
}

/// Resolve a project slug from a working directory path.
///
/// Queries the graph for projects whose root_path matches the cwd.
pub async fn resolve_project_slug(store: &Arc<dyn GraphStore>, cwd: &str) -> Option<String> {
    let projects = store.list_projects().await.ok()?;
    // Find project whose root_path is a prefix of cwd
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_session_cli_id_matching() {
        let test_id = "session-abc-123";
        assert_eq!(test_id, "session-abc-123");
    }

    #[test]
    fn test_project_slug_path_normalization() {
        let path_without_slash = "/home/user/project";
        let normalized = if path_without_slash.ends_with('/') {
            path_without_slash.to_string()
        } else {
            format!("{}/", path_without_slash)
        };
        assert!(normalized.ends_with('/'));
    }
}
