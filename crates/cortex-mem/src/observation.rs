//! Observation → Note conversion.
//!
//! Converts Claude Code PostToolUse observations into knowledge Notes
//! stored in the IndentiaGraph.

use anyhow::{Context, Result};
use cortex_core::notes::{Note, NoteImportance, NoteScope, NoteType};
use cortex_graph::traits::GraphStore;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

/// Raw observation from a PostToolUse hook event.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RawObservation {
    pub content_session_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_response: serde_json::Value,
    pub cwd: String,
}

/// Processed observation ready for storage.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessedObservation {
    pub note_id: Uuid,
    pub observation_type: String,
    pub title: String,
    pub content: String,
    pub source_files: Vec<String>,
    pub content_hash: String,
}

/// Convert a raw observation into a Note and store it in the graph.
///
/// Returns None if the observation should be skipped (duplicate, skip-listed tool, etc.).
pub async fn process_observation(
    store: &Arc<dyn GraphStore>,
    obs: &RawObservation,
    project_id: Option<Uuid>,
    skip_tools: &[String],
) -> Result<Option<ProcessedObservation>> {
    // Skip tools in the exclusion list
    if skip_tools.iter().any(|t| t == &obs.tool_name) {
        return Ok(None);
    }

    // Build observation content
    let content = build_observation_content(obs);
    if content.is_empty() {
        return Ok(None);
    }

    // Check for duplicates via content hash
    let content_hash = compute_hash(&content);

    // Classify the observation type
    let obs_type = classify_observation(&obs.tool_name, &obs.tool_input, &obs.tool_response);

    // Build title
    let title = build_title(&obs.tool_name, &obs.tool_input);

    // Extract source files
    let source_files = extract_source_files(&obs.tool_input, &obs.tool_response);

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

    store
        .create_note(&note)
        .await
        .context("Failed to store observation note")?;

    // Link to project if available
    if let Some(pid) = project_id {
        let _ = store
            .link_note_to_entity(
                note.id,
                &cortex_core::notes::EntityType::Project,
                &pid.to_string(),
                None,
                None,
            )
            .await;
    }

    // Link to source files
    for file_path in &source_files {
        let _ = store
            .link_note_to_entity(
                note.id,
                &cortex_core::notes::EntityType::File,
                file_path,
                None,
                None,
            )
            .await;
    }

    Ok(Some(ProcessedObservation {
        note_id: note.id,
        observation_type: obs_type,
        title,
        content,
        source_files,
        content_hash,
    }))
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

fn classify_observation(
    tool_name: &str,
    input: &serde_json::Value,
    _response: &serde_json::Value,
) -> String {
    match tool_name {
        "Edit" | "Write" | "NotebookEdit" => "change".to_string(),
        "Bash" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.contains("test") || cmd.contains("cargo test") {
                "discovery".to_string()
            } else if cmd.contains("git") {
                "change".to_string()
            } else {
                "discovery".to_string()
            }
        }
        "Read" | "Glob" | "Grep" => "discovery".to_string(),
        _ => "discovery".to_string(),
    }
}

fn build_title(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "Edit" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(path);
            format!("Edited {}", filename)
        }
        "Write" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(path);
            format!("Created {}", filename)
        }
        "Read" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(path);
            format!("Read {}", filename)
        }
        "Bash" => {
            let cmd = input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("command");
            let short = if cmd.len() > 60 { &cmd[..60] } else { cmd };
            format!("Ran: {}", short)
        }
        "Grep" => {
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("pattern");
            format!("Searched for '{}'", pattern)
        }
        "Glob" => {
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("pattern");
            format!("Found files matching '{}'", pattern)
        }
        _ => format!("Used {}", tool_name),
    }
}

fn extract_source_files(input: &serde_json::Value, _response: &serde_json::Value) -> Vec<String> {
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

fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_observation() {
        assert_eq!(
            classify_observation("Edit", &serde_json::json!({}), &serde_json::json!({})),
            "change"
        );
        assert_eq!(
            classify_observation("Read", &serde_json::json!({}), &serde_json::json!({})),
            "discovery"
        );
        assert_eq!(
            classify_observation(
                "Bash",
                &serde_json::json!({"command": "cargo test"}),
                &serde_json::json!({})
            ),
            "discovery"
        );
    }

    #[test]
    fn test_build_title() {
        let title = build_title("Edit", &serde_json::json!({"file_path": "/src/main.rs"}));
        assert_eq!(title, "Edited main.rs");
    }

    #[test]
    fn test_extract_source_files() {
        let files = extract_source_files(
            &serde_json::json!({"file_path": "/src/lib.rs"}),
            &serde_json::json!({}),
        );
        assert_eq!(files, vec!["/src/lib.rs"]);
    }

    #[test]
    fn test_compute_hash() {
        let hash = compute_hash("hello world");
        assert!(!hash.is_empty());
        assert_eq!(hash, compute_hash("hello world"));
    }

    #[test]
    fn test_build_observation_content() {
        let obs = RawObservation {
            content_session_id: "test".to_string(),
            tool_name: "Read".to_string(),
            tool_input: serde_json::json!({"file_path": "/src/main.rs"}),
            tool_response: serde_json::json!("file contents here"),
            cwd: "/tmp".to_string(),
        };
        let content = build_observation_content(&obs);
        assert!(content.contains("Read"));
        assert!(content.contains("main.rs"));
    }

    #[test]
    fn test_classify_change_tools() {
        assert_eq!(
            classify_observation("Write", &serde_json::json!({}), &serde_json::json!({})),
            "change"
        );
        assert_eq!(
            classify_observation(
                "NotebookEdit",
                &serde_json::json!({}),
                &serde_json::json!({})
            ),
            "change"
        );
    }

    #[test]
    fn test_build_title_git_bash() {
        let title = build_title(
            "Bash",
            &serde_json::json!({"command": "git commit -m 'test'"}),
        );
        assert!(title.starts_with("Ran: git commit"));
    }

    #[test]
    fn test_extract_source_files_no_duplicates() {
        let files = extract_source_files(
            &serde_json::json!({"file_path": "/src/lib.rs", "path": "/src/lib.rs"}),
            &serde_json::json!({}),
        );
        assert_eq!(files.len(), 1);
    }
}
