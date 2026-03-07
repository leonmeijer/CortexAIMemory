//! Context injection for SessionStart hooks.
//!
//! Queries the IndentiaGraph for recent observations, session summaries,
//! and relevant notes, then formats them as markdown for injection into
//! Claude Code's context.

use anyhow::Result;
use cortex_core::notes::{NoteFilters, NoteStatus, NoteType};
use cortex_graph::traits::GraphStore;
use std::sync::Arc;

/// Generate context markdown for a project.
///
/// Returns formatted markdown containing:
/// - Recent observations (as notes)
/// - Last session summary (if available)
/// - Relevant knowledge notes
pub async fn generate_context(
    store: &Arc<dyn GraphStore>,
    project_slug: Option<&str>,
    max_observations: usize,
    show_last_summary: bool,
    max_tokens: usize,
) -> Result<String> {
    let mut sections = Vec::new();

    // Resolve project ID from slug
    let project_id = if let Some(slug) = project_slug {
        store
            .get_project_by_slug(slug)
            .await
            .ok()
            .flatten()
            .map(|p| p.id)
    } else {
        None
    };

    // 1. Recent observations (notes of type Observation)
    if let Some(pid) = project_id {
        let filters = NoteFilters {
            note_type: Some(vec![NoteType::Observation]),
            status: Some(vec![NoteStatus::Active]),
            limit: Some(max_observations as i64),
            ..Default::default()
        };
        if let Ok((notes, _total)) = store.list_notes(Some(pid), None, &filters).await {
            if !notes.is_empty() {
                let mut obs_section = String::from("## Recent Observations\n\n");
                for (i, note) in notes.iter().enumerate().take(max_observations) {
                    let age = chrono::Utc::now().signed_duration_since(note.created_at);
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
                    obs_section.push_str(&format!("{}. **{}** ({})\n", i + 1, title, age_str));
                }
                sections.push(obs_section);
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
                if let Some(summary_note) = notes.first() {
                    let mut summary_section = String::from("## Last Session Summary\n\n");
                    summary_section.push_str(&summary_note.content);
                    summary_section.push('\n');
                    sections.push(summary_section);
                }
            }
        }
    }

    // 3. Active knowledge notes (guidelines, gotchas, patterns)
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
            let mut knowledge_section = String::from("## Knowledge Notes\n\n");
            for note in &knowledge_notes {
                let type_label = format!("{:?}", note.note_type).to_lowercase();
                let first_line = note
                    .content
                    .lines()
                    .next()
                    .unwrap_or("Note")
                    .trim_start_matches('#')
                    .trim();
                knowledge_section.push_str(&format!("- **[{}]** {}\n", type_label, first_line));
            }
            sections.push(knowledge_section);
        }
    }

    if sections.is_empty() {
        return Ok(String::new());
    }

    let mut output = String::from("# Memory Context (cortex-mem)\n\n");

    // Token budgeting: prioritize knowledge > summary > observations
    // Rough estimate: 4 chars ≈ 1 token
    let max_chars = max_tokens * 4;
    let header_len = output.len();

    // Sections are in order: observations, summary, knowledge
    // Reverse priority: add knowledge first, then summary, then fill with observations
    let mut budget_sections = Vec::new();
    let mut used = header_len;

    // Knowledge notes (highest priority) — last section if present
    if sections.len() >= 2 {
        for section in sections.iter().rev() {
            if section.starts_with("## Knowledge") || section.starts_with("## Last Session") {
                if used + section.len() <= max_chars {
                    budget_sections.push(section.clone());
                    used += section.len();
                }
            }
        }
    }

    // Observations (fill remaining budget)
    for section in &sections {
        if section.starts_with("## Recent") && used + section.len() <= max_chars {
            budget_sections.push(section.clone());
        } else if section.starts_with("## Recent") && used < max_chars {
            // Truncate observations to fit
            let remaining = max_chars - used;
            let truncated: String = section.chars().take(remaining).collect();
            if let Some(last_newline) = truncated.rfind('\n') {
                budget_sections.push(truncated[..last_newline].to_string());
            }
        }
    }

    // Sort back to logical order: observations, summary, knowledge
    budget_sections.sort_by_key(|s| {
        if s.starts_with("## Recent") {
            0
        } else if s.starts_with("## Last") {
            1
        } else {
            2
        }
    });

    output.push_str(&budget_sections.join("\n"));
    Ok(output)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_context_module_compiles() {
        assert!(true);
    }
}
