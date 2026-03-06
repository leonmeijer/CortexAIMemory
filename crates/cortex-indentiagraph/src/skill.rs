//! Neural Skills CRUD operations for IndentiaGraphStore.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::DecisionNode;
use cortex_core::neurons::ActivatedNote;
use cortex_core::notes::Note;
use cortex_core::skills::{
    ActivatedSkillContext, SkillNode, SkillStatus, SkillTrigger, TriggerType,
};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

// ---------------------------------------------------------------------------
// Record types (module-level for SurrealValue derive)
// ---------------------------------------------------------------------------

/// Skill record matching the SurrealDB `skill` table schema.
///
/// `tags` and `triggers` are stored as `option<array>` in SurrealDB.
/// - `tags`: `Vec<String>` directly
/// - `triggers`: `Vec<String>` where each element is a JSON-serialized SkillTrigger
/// - `centroid_embedding`: stored as `option<array>` of floats, read as Option<Vec<f64>>
#[derive(Debug, SurrealValue)]
struct SkillRecord {
    id: RecordId,
    name: String,
    description: Option<String>,
    status: String,
    project_id: Option<String>,
    energy: f64,
    cohesion: f64,
    activation_count: i64,
    centroid_embedding: Option<Vec<f64>>,
    tags: Option<Vec<String>>,
    triggers: Option<Vec<String>>,
    content_template: Option<String>,
    created_at: String,
    updated_at: Option<String>,
    last_activated_at: Option<String>,
}

/// Lightweight record for has_member edge queries.
#[derive(Debug, SurrealValue)]
struct HasMemberEdge {
    out: RecordId,
    entity_type: Option<String>,
}

/// Lightweight record for note table queries used in get_skill_members.
#[derive(Debug, SurrealValue)]
struct NoteRecord {
    id: RecordId,
    project_id: Option<String>,
    note_type: String,
    status: String,
    importance: String,
    content: String,
    tags: Option<Vec<String>>,
    scope_type: Option<String>,
    scope_path: Option<String>,
    staleness_score: Option<f64>,
    energy: Option<f64>,
    created_at: String,
    updated_at: Option<String>,
    confirmed_at: Option<String>,
}

/// Lightweight record for decision table queries used in get_skill_members.
#[derive(Debug, SurrealValue)]
struct DecisionRecord {
    id: RecordId,
    description: String,
    rationale: String,
    alternatives: Option<String>,
    chosen_option: Option<String>,
    decided_by: String,
    decided_at: String,
    status: String,
}

/// Record for counting rows.
#[derive(Debug, SurrealValue)]
struct CountRecord {
    count: i64,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn parse_skill_status(s: &str) -> SkillStatus {
    match s {
        "emerging" => SkillStatus::Emerging,
        "active" => SkillStatus::Active,
        "dormant" => SkillStatus::Dormant,
        "archived" => SkillStatus::Archived,
        "imported" => SkillStatus::Imported,
        _ => SkillStatus::Emerging,
    }
}

fn status_to_str(s: &SkillStatus) -> &'static str {
    match s {
        SkillStatus::Emerging => "emerging",
        SkillStatus::Active => "active",
        SkillStatus::Dormant => "dormant",
        SkillStatus::Archived => "archived",
        SkillStatus::Imported => "imported",
    }
}

fn parse_decision_status(s: &str) -> cortex_core::models::DecisionStatus {
    match s {
        "proposed" => cortex_core::models::DecisionStatus::Proposed,
        "accepted" => cortex_core::models::DecisionStatus::Accepted,
        "deprecated" => cortex_core::models::DecisionStatus::Deprecated,
        "superseded" => cortex_core::models::DecisionStatus::Superseded,
        _ => cortex_core::models::DecisionStatus::Proposed,
    }
}

fn parse_note_type(s: &str) -> cortex_core::notes::NoteType {
    use cortex_core::notes::NoteType;
    match s {
        "guideline" => NoteType::Guideline,
        "gotcha" => NoteType::Gotcha,
        "pattern" => NoteType::Pattern,
        "context" => NoteType::Context,
        "tip" => NoteType::Tip,
        "observation" => NoteType::Observation,
        "assertion" => NoteType::Assertion,
        _ => NoteType::Observation,
    }
}

fn parse_note_status(s: &str) -> cortex_core::notes::NoteStatus {
    use cortex_core::notes::NoteStatus;
    match s {
        "active" => NoteStatus::Active,
        "needs_review" => NoteStatus::NeedsReview,
        "stale" => NoteStatus::Stale,
        "obsolete" => NoteStatus::Obsolete,
        "archived" => NoteStatus::Archived,
        _ => NoteStatus::Active,
    }
}

fn parse_note_importance(s: &str) -> cortex_core::notes::NoteImportance {
    use cortex_core::notes::NoteImportance;
    match s {
        "low" => NoteImportance::Low,
        "medium" => NoteImportance::Medium,
        "high" => NoteImportance::High,
        "critical" => NoteImportance::Critical,
        _ => NoteImportance::Medium,
    }
}

fn parse_note_scope(
    scope_type: Option<&str>,
    scope_path: Option<&str>,
) -> cortex_core::notes::NoteScope {
    use cortex_core::notes::NoteScope;
    match scope_type.unwrap_or("project") {
        "workspace" => NoteScope::Workspace,
        "project" => NoteScope::Project,
        "module" => NoteScope::Module(scope_path.unwrap_or("").to_string()),
        "file" => NoteScope::File(scope_path.unwrap_or("").to_string()),
        "function" => NoteScope::Function(scope_path.unwrap_or("").to_string()),
        "struct" => NoteScope::Struct(scope_path.unwrap_or("").to_string()),
        "trait" => NoteScope::Trait(scope_path.unwrap_or("").to_string()),
        _ => NoteScope::Project,
    }
}

/// Serialize triggers to a Vec of JSON strings (one per trigger).
fn triggers_to_string_vec(triggers: &[SkillTrigger]) -> Option<Vec<String>> {
    if triggers.is_empty() {
        None
    } else {
        Some(
            triggers
                .iter()
                .map(|t| serde_json::to_string(t).unwrap_or_default())
                .collect(),
        )
    }
}

/// Deserialize triggers from a Vec of JSON strings.
fn triggers_from_string_vec(v: &Option<Vec<String>>) -> Vec<SkillTrigger> {
    v.as_ref()
        .map(|arr| {
            arr.iter()
                .filter_map(|s| serde_json::from_str(s).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Convert tags Vec to Option (None if empty).
fn tags_to_option(tags: &[String]) -> Option<Vec<String>> {
    if tags.is_empty() {
        None
    } else {
        Some(tags.to_vec())
    }
}

impl SkillRecord {
    fn into_node(self) -> Result<SkillNode> {
        let tags = self.tags.unwrap_or_default();
        let triggers = triggers_from_string_vec(&self.triggers);

        let created_at = self
            .created_at
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| Utc::now());

        let updated_at = self
            .updated_at
            .as_ref()
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or(created_at);

        let last_activated = self
            .last_activated_at
            .as_ref()
            .and_then(|s| s.parse::<DateTime<Utc>>().ok());

        let project_id = self
            .project_id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_default();

        Ok(SkillNode {
            id: rid_to_uuid(&self.id)?,
            project_id,
            name: self.name,
            description: self.description.unwrap_or_default(),
            status: parse_skill_status(&self.status),
            trigger_patterns: triggers,
            context_template: self.content_template,
            energy: self.energy,
            cohesion: self.cohesion,
            coverage: 0,
            note_count: 0,
            decision_count: 0,
            activation_count: self.activation_count,
            hit_rate: 0.0,
            last_activated,
            version: 1,
            fingerprint: None,
            imported_at: None,
            is_validated: false,
            tags,
            created_at,
            updated_at,
        })
    }
}

impl NoteRecord {
    fn into_note(self) -> Result<Note> {
        let tags = self.tags.unwrap_or_default();

        let project_id = self
            .project_id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok());

        let created_at = self
            .created_at
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| Utc::now());

        let last_confirmed_at = self
            .confirmed_at
            .as_ref()
            .and_then(|s| s.parse::<DateTime<Utc>>().ok());

        let scope = parse_note_scope(self.scope_type.as_deref(), self.scope_path.as_deref());

        Ok(Note {
            id: rid_to_uuid(&self.id)?,
            project_id,
            note_type: parse_note_type(&self.note_type),
            status: parse_note_status(&self.status),
            importance: parse_note_importance(&self.importance),
            scope,
            content: self.content,
            tags,
            anchors: vec![],
            created_at,
            created_by: String::new(),
            last_confirmed_at,
            last_confirmed_by: None,
            staleness_score: self.staleness_score.unwrap_or(0.0),
            energy: self.energy.unwrap_or(1.0),
            last_activated: None,
            supersedes: None,
            superseded_by: None,
            changes: vec![],
            valid_at: None,
            invalid_at: None,
            assertion_rule: None,
            last_assertion_result: None,
        })
    }
}

impl DecisionRecord {
    fn into_decision(self) -> Result<DecisionNode> {
        let alternatives: Vec<String> = self
            .alternatives
            .as_ref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        Ok(DecisionNode {
            id: rid_to_uuid(&self.id)?,
            description: self.description,
            rationale: self.rationale,
            alternatives,
            chosen_option: self.chosen_option,
            decided_by: self.decided_by,
            decided_at: self
                .decided_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            status: parse_decision_status(&self.status),
            embedding: None,
            embedding_model: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl IndentiaGraphStore {
    // -----------------------------------------------------------------------
    // Core CRUD
    // -----------------------------------------------------------------------

    /// Create a skill node.
    pub async fn create_skill(&self, skill: &SkillNode) -> Result<()> {
        let rid = RecordId::new("skill", skill.id.to_string().as_str());

        self.db
            .query(
                "CREATE $rid SET \
                 name = $name, description = $desc, status = $status, \
                 project_id = $pid, energy = $energy, cohesion = $cohesion, \
                 activation_count = $ac, centroid_embedding = NONE, \
                 tags = $tags, triggers = $triggers, \
                 content_template = $ct, \
                 created_at = $cat, updated_at = $uat, \
                 last_activated_at = $laa \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("name", skill.name.clone()))
            .bind(("desc", Some(skill.description.clone())))
            .bind(("status", status_to_str(&skill.status).to_string()))
            .bind(("pid", Some(skill.project_id.to_string())))
            .bind(("energy", skill.energy))
            .bind(("cohesion", skill.cohesion))
            .bind(("ac", skill.activation_count))
            .bind(("tags", tags_to_option(&skill.tags)))
            .bind(("triggers", triggers_to_string_vec(&skill.trigger_patterns)))
            .bind(("ct", skill.context_template.clone()))
            .bind(("cat", skill.created_at.to_rfc3339()))
            .bind(("uat", Some(skill.updated_at.to_rfc3339())))
            .bind(("laa", skill.last_activated.map(|dt| dt.to_rfc3339())))
            .await
            .context("Failed to create skill")?;

        Ok(())
    }

    /// Get a skill by ID.
    pub async fn get_skill(&self, id: Uuid) -> Result<Option<SkillNode>> {
        let rid = RecordId::new("skill", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get skill")?;
        let records: Vec<SkillRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    /// Update a skill node (full update).
    pub async fn update_skill(&self, skill: &SkillNode) -> Result<()> {
        let rid = RecordId::new("skill", skill.id.to_string().as_str());

        self.db
            .query(
                "UPDATE $rid SET \
                 name = $name, description = $desc, status = $status, \
                 project_id = $pid, energy = $energy, cohesion = $cohesion, \
                 activation_count = $ac, \
                 tags = $tags, triggers = $triggers, \
                 content_template = $ct, \
                 updated_at = $uat, \
                 last_activated_at = $laa \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("name", skill.name.clone()))
            .bind(("desc", Some(skill.description.clone())))
            .bind(("status", status_to_str(&skill.status).to_string()))
            .bind(("pid", Some(skill.project_id.to_string())))
            .bind(("energy", skill.energy))
            .bind(("cohesion", skill.cohesion))
            .bind(("ac", skill.activation_count))
            .bind(("tags", tags_to_option(&skill.tags)))
            .bind(("triggers", triggers_to_string_vec(&skill.trigger_patterns)))
            .bind(("ct", skill.context_template.clone()))
            .bind(("uat", Some(skill.updated_at.to_rfc3339())))
            .bind(("laa", skill.last_activated.map(|dt| dt.to_rfc3339())))
            .await
            .context("Failed to update skill")?;

        Ok(())
    }

    /// Delete a skill and its has_member edges. Returns true if the skill existed.
    pub async fn delete_skill(&self, id: Uuid) -> Result<bool> {
        // First check if it exists
        let existing = self.get_skill(id).await?;
        if existing.is_none() {
            return Ok(false);
        }

        let sid = id.to_string();
        let rid = RecordId::new("skill", sid.as_str());

        self.db
            .query(
                "DELETE FROM has_member WHERE in = type::record('skill', $sid);\
                 DELETE $rid",
            )
            .bind(("sid", sid))
            .bind(("rid", rid))
            .await
            .context("Failed to delete skill")?;

        Ok(true)
    }

    /// List skills for a project with optional status filter, pagination.
    pub async fn list_skills(
        &self,
        project_id: Uuid,
        status: Option<SkillStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<SkillNode>, usize)> {
        let pid = project_id.to_string();

        let (count_sql, data_sql) = if status.is_some() {
            (
                "SELECT count() AS count FROM skill \
                 WHERE project_id = $pid AND status = $status GROUP ALL",
                "SELECT * FROM skill WHERE project_id = $pid AND status = $status \
                 ORDER BY created_at DESC LIMIT $limit START $offset",
            )
        } else {
            (
                "SELECT count() AS count FROM skill WHERE project_id = $pid GROUP ALL",
                "SELECT * FROM skill WHERE project_id = $pid \
                 ORDER BY created_at DESC LIMIT $limit START $offset",
            )
        };

        let combined = format!("{count_sql}; {data_sql}");
        let mut qb = self
            .db
            .query(&combined)
            .bind(("pid", pid))
            .bind(("limit", limit as i64))
            .bind(("offset", offset as i64));
        if let Some(ref s) = status {
            qb = qb.bind(("status", status_to_str(s).to_string()));
        }
        let mut resp = qb.await.context("Failed to list skills")?;

        let count_records: Vec<CountRecord> = resp.take(0)?;
        let total = count_records
            .into_iter()
            .next()
            .map(|r| r.count as usize)
            .unwrap_or(0);
        let records: Vec<SkillRecord> = resp.take(1)?;
        let skills: Result<Vec<SkillNode>> = records.into_iter().map(|r| r.into_node()).collect();

        Ok((skills?, total))
    }

    // -----------------------------------------------------------------------
    // Membership
    // -----------------------------------------------------------------------

    /// Get member notes and decisions for a skill.
    pub async fn get_skill_members(
        &self,
        skill_id: Uuid,
    ) -> Result<(Vec<Note>, Vec<DecisionNode>)> {
        let sid = skill_id.to_string();

        // Query has_member edges to get entity types and target record IDs
        let mut resp = self
            .db
            .query(
                "SELECT out, entity_type FROM has_member \
                 WHERE in = type::record('skill', $sid)",
            )
            .bind(("sid", sid))
            .await
            .context("Failed to get skill members")?;
        let edges: Vec<HasMemberEdge> = resp.take(0)?;

        let mut notes = Vec::new();
        let mut decisions = Vec::new();

        for edge in edges {
            let entity_type = edge.entity_type.as_deref().unwrap_or("note");
            let entity_id = rid_to_uuid(&edge.out)?;

            match entity_type {
                "note" => {
                    let rid = RecordId::new("note", entity_id.to_string().as_str());
                    let mut note_resp = self
                        .db
                        .query("SELECT * FROM $rid")
                        .bind(("rid", rid))
                        .await
                        .context("Failed to get note member")?;
                    let note_records: Vec<NoteRecord> = note_resp.take(0)?;
                    if let Some(nr) = note_records.into_iter().next() {
                        notes.push(nr.into_note()?);
                    }
                }
                "decision" => {
                    let rid = RecordId::new("decision", entity_id.to_string().as_str());
                    let mut dec_resp = self
                        .db
                        .query("SELECT * FROM $rid")
                        .bind(("rid", rid))
                        .await
                        .context("Failed to get decision member")?;
                    let dec_records: Vec<DecisionRecord> = dec_resp.take(0)?;
                    if let Some(dr) = dec_records.into_iter().next() {
                        decisions.push(dr.into_decision()?);
                    }
                }
                _ => {} // ignore unknown entity types
            }
        }

        Ok((notes, decisions))
    }

    /// Add a member (note or decision) to a skill via has_member edge.
    pub async fn add_skill_member(
        &self,
        skill_id: Uuid,
        entity_type: &str,
        entity_id: Uuid,
    ) -> Result<()> {
        let skill_rid = RecordId::new("skill", skill_id.to_string().as_str());
        let target_rid = RecordId::new(entity_type, entity_id.to_string().as_str());

        self.db
            .query("RELATE $from->has_member->$to SET entity_type = $et RETURN NONE")
            .bind(("from", skill_rid))
            .bind(("to", target_rid))
            .bind(("et", entity_type.to_string()))
            .await
            .context("Failed to add skill member")?;

        Ok(())
    }

    /// Remove a member from a skill. Returns true if an edge was deleted.
    pub async fn remove_skill_member(
        &self,
        skill_id: Uuid,
        entity_type: &str,
        entity_id: Uuid,
    ) -> Result<bool> {
        let skill_rid = RecordId::new("skill", skill_id.to_string().as_str());
        let entity_rid = RecordId::new(entity_type, entity_id.to_string().as_str());

        // Count before
        let mut count_resp = self
            .db
            .query(
                "SELECT count() AS count FROM has_member \
                 WHERE in = $skill_rid AND out = $entity_rid GROUP ALL",
            )
            .bind(("skill_rid", skill_rid.clone()))
            .bind(("entity_rid", entity_rid.clone()))
            .await
            .context("Failed to count skill members")?;
        let count_records: Vec<CountRecord> = count_resp.take(0)?;
        let before = count_records
            .into_iter()
            .next()
            .map(|r| r.count)
            .unwrap_or(0);

        // Delete
        self.db
            .query("DELETE FROM has_member WHERE in = $skill_rid AND out = $entity_rid")
            .bind(("skill_rid", skill_rid))
            .bind(("entity_rid", entity_rid))
            .await
            .context("Failed to remove skill member")?;

        Ok(before > 0)
    }

    /// Remove all members from a skill. Returns the count of removed edges.
    pub async fn remove_all_skill_members(&self, skill_id: Uuid) -> Result<i64> {
        let skill_rid = RecordId::new("skill", skill_id.to_string().as_str());

        // Count before
        let mut count_resp = self
            .db
            .query(
                "SELECT count() AS count FROM has_member \
                 WHERE in = $skill_rid GROUP ALL",
            )
            .bind(("skill_rid", skill_rid.clone()))
            .await
            .context("Failed to count skill members")?;
        let count_records: Vec<CountRecord> = count_resp.take(0)?;
        let before = count_records
            .into_iter()
            .next()
            .map(|r| r.count)
            .unwrap_or(0);

        // Delete all
        self.db
            .query("DELETE FROM has_member WHERE in = $skill_rid")
            .bind(("skill_rid", skill_rid))
            .await
            .context("Failed to remove all skill members")?;

        Ok(before)
    }

    // -----------------------------------------------------------------------
    // Query & Activation
    // -----------------------------------------------------------------------

    /// Get all skills that contain a specific note (via has_member edge).
    pub async fn get_skills_for_note(&self, note_id: Uuid) -> Result<Vec<SkillNode>> {
        let note_rid = RecordId::new("note", note_id.to_string().as_str());
        let mut resp = self
            .db
            .query(
                "SELECT * FROM skill WHERE id IN \
                 (SELECT VALUE in.id FROM has_member \
                  WHERE out = $note_rid AND entity_type = 'note')",
            )
            .bind(("note_rid", note_rid))
            .await
            .context("Failed to get skills for note")?;
        let records: Vec<SkillRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    /// Get all skills for a project (any status).
    pub async fn get_skills_for_project(&self, project_id: Uuid) -> Result<Vec<SkillNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM skill WHERE project_id = $pid ORDER BY energy DESC")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to get skills for project")?;
        let records: Vec<SkillRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    /// Activate a skill: return the skill with its member notes and decisions.
    ///
    /// The `context_text` is assembled from member content. Confidence is 1.0
    /// for direct activation.
    pub async fn activate_skill(
        &self,
        skill_id: Uuid,
        _query: &str,
    ) -> Result<ActivatedSkillContext> {
        let skill = self.get_skill(skill_id).await?.context("Skill not found")?;

        let (notes, decisions) = self.get_skill_members(skill_id).await?;

        // Build context text from members
        let mut context_parts = Vec::new();
        context_parts.push(format!("## {}", skill.name));
        if !skill.description.is_empty() {
            context_parts.push(skill.description.clone());
        }

        if !notes.is_empty() {
            context_parts.push("\n### Notes".to_string());
            for note in &notes {
                context_parts.push(format!("- [{}] {}", note.note_type, note.content));
            }
        }

        if !decisions.is_empty() {
            context_parts.push("\n### Decisions".to_string());
            for dec in &decisions {
                context_parts.push(format!("- {}: {}", dec.description, dec.rationale));
            }
        }

        let context_text = context_parts.join("\n");

        // Convert notes to ActivatedNote
        let activated_notes: Vec<ActivatedNote> = notes
            .into_iter()
            .map(|note| ActivatedNote {
                note,
                activation_score: 1.0,
                source: cortex_core::neurons::ActivationSource::Direct,
                entity_type: "note".to_string(),
            })
            .collect();

        Ok(ActivatedSkillContext {
            skill,
            activated_notes,
            relevant_decisions: decisions,
            context_text,
            confidence: 1.0,
        })
    }

    /// Increment the activation count and update last_activated_at for a skill.
    pub async fn increment_skill_activation(&self, skill_id: Uuid) -> Result<()> {
        let rid = RecordId::new("skill", skill_id.to_string().as_str());
        let now = Utc::now().to_rfc3339();

        self.db
            .query(
                "UPDATE $rid SET \
                 activation_count = activation_count + 1, \
                 last_activated_at = $now \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("now", now))
            .await
            .context("Failed to increment skill activation")?;

        Ok(())
    }

    /// Match skills by trigger patterns against an input string.
    ///
    /// Loads all active/emerging skills for the project, evaluates triggers
    /// against the input, and returns matched skills with confidence scores.
    pub async fn match_skills_by_trigger(
        &self,
        project_id: Uuid,
        input: &str,
    ) -> Result<Vec<(SkillNode, f64)>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM skill WHERE project_id = $pid \
                 AND (status = 'active' OR status = 'emerging')",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to load skills for trigger matching")?;
        let records: Vec<SkillRecord> = resp.take(0)?;
        let skills: Vec<SkillNode> = records
            .into_iter()
            .filter_map(|r| r.into_node().ok())
            .collect();

        let mut matches = Vec::new();

        for skill in skills {
            let mut best_confidence: Option<f64> = None;

            for trigger in skill.reliable_triggers() {
                let confidence = match trigger.pattern_type {
                    TriggerType::Regex => match regex::Regex::new(&trigger.pattern_value) {
                        Ok(re) => {
                            if re.is_match(input) {
                                Some(1.0_f64.min(trigger.confidence_threshold.max(0.8)))
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    },
                    TriggerType::FileGlob => match glob::Pattern::new(&trigger.pattern_value) {
                        Ok(pattern) => {
                            if pattern.matches(input) {
                                Some(trigger.confidence_threshold.max(0.7))
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    },
                    TriggerType::Semantic => {
                        // Semantic triggers need embeddings; return placeholder confidence
                        Some(0.5)
                    }
                    TriggerType::McpAction => {
                        // Check if input starts with the pattern or matches exactly
                        if input == trigger.pattern_value
                            || input.starts_with(&format!("{}:", trigger.pattern_value))
                        {
                            Some(trigger.confidence_threshold.max(0.9))
                        } else {
                            None
                        }
                    }
                };

                if let Some(conf) = confidence {
                    if conf >= trigger.confidence_threshold {
                        best_confidence =
                            Some(best_confidence.map(|prev| prev.max(conf)).unwrap_or(conf));
                    }
                }
            }

            if let Some(conf) = best_confidence {
                matches.push((skill, conf));
            }
        }

        // Sort by confidence descending
        matches.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(matches)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::models::DecisionNode;
    use cortex_core::notes::{Note, NoteType};
    use cortex_core::skills::{SkillNode, SkillStatus, SkillTrigger};
    use cortex_core::test_helpers::test_decision;

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    fn test_skill(project_id: Uuid) -> SkillNode {
        SkillNode::new_full(
            project_id,
            "IndentiaGraph Performance",
            "Optimization patterns for IndentiaGraph queries",
            0.72,
            0.68,
            vec!["indentiagraph".into(), "performance".into()],
        )
    }

    fn test_note(project_id: Uuid) -> Note {
        Note::new(
            Some(project_id),
            NoteType::Guideline,
            "Always use UNWIND for batch operations".to_string(),
            "test-agent".to_string(),
        )
    }

    /// Helper to create a note in the store.
    async fn create_note_in_store(store: &IndentiaGraphStore, note: &Note) {
        let rid = RecordId::new("note", note.id.to_string().as_str());
        let tags: Option<Vec<String>> = if note.tags.is_empty() {
            None
        } else {
            Some(note.tags.clone())
        };
        store
            .db
            .query(
                "CREATE $rid SET \
                 project_id = $pid, note_type = $nt, status = $status, \
                 importance = $imp, content = $content, tags = $tags, \
                 scope_type = 'project', scope_path = NONE, \
                 staleness_score = 0.0, energy = 1.0, \
                 code_anchor_hash = NONE, \
                 created_at = $cat, updated_at = NONE, confirmed_at = NONE, \
                 embedding = NONE, embedding_model = NONE \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("pid", note.project_id.map(|id| id.to_string())))
            .bind(("nt", note.note_type.to_string()))
            .bind(("status", note.status.to_string()))
            .bind(("imp", note.importance.to_string()))
            .bind(("content", note.content.clone()))
            .bind(("tags", tags))
            .bind(("cat", note.created_at.to_rfc3339()))
            .await
            .unwrap();
    }

    /// Helper to create a decision in the store (without task linkage).
    async fn create_decision_in_store(store: &IndentiaGraphStore, decision: &DecisionNode) {
        let rid = RecordId::new("decision", decision.id.to_string().as_str());
        let alts_json = if decision.alternatives.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&decision.alternatives).unwrap())
        };
        store
            .db
            .query(
                "CREATE $rid SET \
                 description = $desc, rationale = $rat, \
                 alternatives = $alts, chosen_option = $co, \
                 decided_by = $db, decided_at = $da, \
                 status = $status, task_id = NONE, \
                 embedding = NONE, embedding_model = NONE \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("desc", decision.description.clone()))
            .bind(("rat", decision.rationale.clone()))
            .bind(("alts", alts_json))
            .bind(("co", decision.chosen_option.clone()))
            .bind(("db", decision.decided_by.clone()))
            .bind(("da", decision.decided_at.to_rfc3339()))
            .bind(("status", "proposed".to_string()))
            .await
            .unwrap();
    }

    // -----------------------------------------------------------------------
    // CRUD tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_create_and_get_skill() {
        let store = setup().await;
        let project_id = Uuid::new_v4();
        let skill = test_skill(project_id);

        store.create_skill(&skill).await.unwrap();

        let retrieved = store.get_skill(skill.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, skill.id);
        assert_eq!(retrieved.name, "IndentiaGraph Performance");
        assert_eq!(
            retrieved.description,
            "Optimization patterns for IndentiaGraph queries"
        );
        assert_eq!(retrieved.status, SkillStatus::Emerging);
        assert!((retrieved.energy - 0.72).abs() < f64::EPSILON);
        assert!((retrieved.cohesion - 0.68).abs() < f64::EPSILON);
        assert_eq!(retrieved.tags, vec!["indentiagraph", "performance"]);
        assert_eq!(retrieved.project_id, project_id);

        // Non-existent skill returns None
        assert!(store.get_skill(Uuid::new_v4()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_update_skill() {
        let store = setup().await;
        let project_id = Uuid::new_v4();
        let mut skill = test_skill(project_id);

        store.create_skill(&skill).await.unwrap();

        // Update fields
        skill.name = "IndentiaGraph Query Optimization".to_string();
        skill.status = SkillStatus::Active;
        skill.energy = 0.95;
        skill.activation_count = 5;
        skill.tags = vec![
            "indentiagraph".into(),
            "cypher".into(),
            "performance".into(),
        ];
        skill.trigger_patterns = vec![SkillTrigger::regex("indentiagraph|cypher", 0.6)];
        skill.updated_at = Utc::now();

        store.update_skill(&skill).await.unwrap();

        let retrieved = store.get_skill(skill.id).await.unwrap().unwrap();
        assert_eq!(retrieved.name, "IndentiaGraph Query Optimization");
        assert_eq!(retrieved.status, SkillStatus::Active);
        assert!((retrieved.energy - 0.95).abs() < f64::EPSILON);
        assert_eq!(retrieved.activation_count, 5);
        assert_eq!(retrieved.tags.len(), 3);
        assert_eq!(retrieved.trigger_patterns.len(), 1);
        assert_eq!(
            retrieved.trigger_patterns[0].pattern_value,
            "indentiagraph|cypher"
        );
    }

    #[tokio::test]
    async fn test_delete_skill() {
        let store = setup().await;
        let project_id = Uuid::new_v4();
        let skill = test_skill(project_id);

        store.create_skill(&skill).await.unwrap();

        // Delete existing skill
        let deleted = store.delete_skill(skill.id).await.unwrap();
        assert!(deleted);

        // Verify it's gone
        assert!(store.get_skill(skill.id).await.unwrap().is_none());

        // Delete non-existent returns false
        let deleted_again = store.delete_skill(skill.id).await.unwrap();
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn test_list_skills() {
        let store = setup().await;
        let project_id = Uuid::new_v4();

        // Create multiple skills
        let mut s1 = test_skill(project_id);
        s1.name = "Skill A".to_string();
        s1.status = SkillStatus::Active;

        let mut s2 = test_skill(project_id);
        s2.name = "Skill B".to_string();
        s2.status = SkillStatus::Emerging;

        let mut s3 = test_skill(project_id);
        s3.name = "Skill C".to_string();
        s3.status = SkillStatus::Active;

        // Different project
        let other_project = Uuid::new_v4();
        let mut s4 = test_skill(other_project);
        s4.name = "Other Skill".to_string();

        store.create_skill(&s1).await.unwrap();
        store.create_skill(&s2).await.unwrap();
        store.create_skill(&s3).await.unwrap();
        store.create_skill(&s4).await.unwrap();

        // List all for project
        let (skills, total) = store.list_skills(project_id, None, 10, 0).await.unwrap();
        assert_eq!(total, 3);
        assert_eq!(skills.len(), 3);

        // List by status
        let (active, active_total) = store
            .list_skills(project_id, Some(SkillStatus::Active), 10, 0)
            .await
            .unwrap();
        assert_eq!(active_total, 2);
        assert_eq!(active.len(), 2);

        // Pagination
        let (page1, total) = store.list_skills(project_id, None, 2, 0).await.unwrap();
        assert_eq!(total, 3);
        assert_eq!(page1.len(), 2);

        let (page2, _) = store.list_skills(project_id, None, 2, 2).await.unwrap();
        assert_eq!(page2.len(), 1);

        // Other project
        let (other, other_total) = store.list_skills(other_project, None, 10, 0).await.unwrap();
        assert_eq!(other_total, 1);
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].name, "Other Skill");
    }

    // -----------------------------------------------------------------------
    // Membership tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_skill_members() {
        let store = setup().await;
        let project_id = Uuid::new_v4();
        let skill = test_skill(project_id);
        store.create_skill(&skill).await.unwrap();

        // Create a note and a decision in the store
        let note = test_note(project_id);
        create_note_in_store(&store, &note).await;

        let decision = test_decision();
        create_decision_in_store(&store, &decision).await;

        // Add members
        store
            .add_skill_member(skill.id, "note", note.id)
            .await
            .unwrap();
        store
            .add_skill_member(skill.id, "decision", decision.id)
            .await
            .unwrap();

        // Get members
        let (notes, decisions) = store.get_skill_members(skill.id).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(decisions.len(), 1);
        assert_eq!(notes[0].id, note.id);
        assert_eq!(decisions[0].id, decision.id);

        // Remove note member
        let removed = store
            .remove_skill_member(skill.id, "note", note.id)
            .await
            .unwrap();
        assert!(removed);

        // Remove non-existent member
        let removed_again = store
            .remove_skill_member(skill.id, "note", note.id)
            .await
            .unwrap();
        assert!(!removed_again);

        // Verify only decision remains
        let (notes_after, decisions_after) = store.get_skill_members(skill.id).await.unwrap();
        assert_eq!(notes_after.len(), 0);
        assert_eq!(decisions_after.len(), 1);

        // Remove all members
        let count = store.remove_all_skill_members(skill.id).await.unwrap();
        assert_eq!(count, 1);

        let (n, d) = store.get_skill_members(skill.id).await.unwrap();
        assert_eq!(n.len(), 0);
        assert_eq!(d.len(), 0);
    }

    // -----------------------------------------------------------------------
    // Query tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_skills_for_note() {
        let store = setup().await;
        let project_id = Uuid::new_v4();

        let skill1 = test_skill(project_id);
        let mut skill2 = test_skill(project_id);
        skill2.name = "API Auth".to_string();

        store.create_skill(&skill1).await.unwrap();
        store.create_skill(&skill2).await.unwrap();

        let note = test_note(project_id);
        create_note_in_store(&store, &note).await;

        // Link note to both skills
        store
            .add_skill_member(skill1.id, "note", note.id)
            .await
            .unwrap();
        store
            .add_skill_member(skill2.id, "note", note.id)
            .await
            .unwrap();

        let skills = store.get_skills_for_note(note.id).await.unwrap();
        assert_eq!(skills.len(), 2);

        let skill_ids: Vec<Uuid> = skills.iter().map(|s| s.id).collect();
        assert!(skill_ids.contains(&skill1.id));
        assert!(skill_ids.contains(&skill2.id));
    }

    #[tokio::test]
    async fn test_skills_for_project() {
        let store = setup().await;
        let project_id = Uuid::new_v4();

        let s1 = test_skill(project_id);
        let mut s2 = test_skill(project_id);
        s2.name = "Auth Patterns".to_string();

        store.create_skill(&s1).await.unwrap();
        store.create_skill(&s2).await.unwrap();

        let skills = store.get_skills_for_project(project_id).await.unwrap();
        assert_eq!(skills.len(), 2);

        // Different project should be empty
        let empty = store.get_skills_for_project(Uuid::new_v4()).await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn test_increment_activation() {
        let store = setup().await;
        let project_id = Uuid::new_v4();
        let skill = test_skill(project_id);
        store.create_skill(&skill).await.unwrap();

        // Initial count is 0
        let before = store.get_skill(skill.id).await.unwrap().unwrap();
        assert_eq!(before.activation_count, 0);

        // Increment
        store.increment_skill_activation(skill.id).await.unwrap();
        store.increment_skill_activation(skill.id).await.unwrap();

        let after = store.get_skill(skill.id).await.unwrap().unwrap();
        assert_eq!(after.activation_count, 2);
        assert!(after.last_activated.is_some());
    }

    // -----------------------------------------------------------------------
    // Activation & trigger matching
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_activate_skill() {
        let store = setup().await;
        let project_id = Uuid::new_v4();
        let skill = test_skill(project_id);
        store.create_skill(&skill).await.unwrap();

        // Add a note member
        let note = test_note(project_id);
        create_note_in_store(&store, &note).await;
        store
            .add_skill_member(skill.id, "note", note.id)
            .await
            .unwrap();

        // Activate
        let ctx = store
            .activate_skill(skill.id, "indentiagraph query")
            .await
            .unwrap();
        assert_eq!(ctx.skill.id, skill.id);
        assert_eq!(ctx.activated_notes.len(), 1);
        assert!((ctx.confidence - 1.0).abs() < f64::EPSILON);
        assert!(ctx.context_text.contains("IndentiaGraph Performance"));
        assert!(ctx.context_text.contains("UNWIND"));
    }

    #[tokio::test]
    async fn test_match_skills_by_regex_trigger() {
        let store = setup().await;
        let project_id = Uuid::new_v4();

        let mut skill = test_skill(project_id);
        skill.status = SkillStatus::Active;
        skill.trigger_patterns = vec![SkillTrigger::regex("indentiagraph|cypher|UNWIND", 0.6)];
        store.create_skill(&skill).await.unwrap();

        // Should match
        let matches = store
            .match_skills_by_trigger(project_id, "How to optimize indentiagraph queries?")
            .await
            .unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0.id, skill.id);
        assert!(matches[0].1 >= 0.6);

        // Should not match
        let no_matches = store
            .match_skills_by_trigger(project_id, "How to use PostgreSQL?")
            .await
            .unwrap();
        assert!(no_matches.is_empty());
    }

    #[tokio::test]
    async fn test_match_skills_by_glob_trigger() {
        let store = setup().await;
        let project_id = Uuid::new_v4();

        let mut skill = test_skill(project_id);
        skill.status = SkillStatus::Active;
        skill.trigger_patterns = vec![SkillTrigger::file_glob("src/indentiagraph/**", 0.7)];
        store.create_skill(&skill).await.unwrap();

        // Should match
        let matches = store
            .match_skills_by_trigger(project_id, "src/indentiagraph/client.rs")
            .await
            .unwrap();
        assert_eq!(matches.len(), 1);

        // Should not match
        let no_matches = store
            .match_skills_by_trigger(project_id, "src/api/routes.rs")
            .await
            .unwrap();
        assert!(no_matches.is_empty());
    }

    #[tokio::test]
    async fn test_match_skills_dormant_excluded() {
        let store = setup().await;
        let project_id = Uuid::new_v4();

        let mut skill = test_skill(project_id);
        skill.status = SkillStatus::Dormant;
        skill.trigger_patterns = vec![SkillTrigger::regex("indentiagraph", 0.5)];
        store.create_skill(&skill).await.unwrap();

        // Dormant skills should not be matched
        let matches = store
            .match_skills_by_trigger(project_id, "indentiagraph query")
            .await
            .unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_delete_skill_cleans_members() {
        let store = setup().await;
        let project_id = Uuid::new_v4();
        let skill = test_skill(project_id);
        store.create_skill(&skill).await.unwrap();

        let note = test_note(project_id);
        create_note_in_store(&store, &note).await;
        store
            .add_skill_member(skill.id, "note", note.id)
            .await
            .unwrap();

        // Delete skill should also clean up member edges
        store.delete_skill(skill.id).await.unwrap();

        // Verify skill is gone
        assert!(store.get_skill(skill.id).await.unwrap().is_none());

        // Verify note still exists (only the edge should be deleted)
        let nid = note.id.to_string();
        let rid = RecordId::new("note", nid.as_str());
        let mut resp = store
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .unwrap();
        let records: Vec<NoteRecord> = resp.take(0).unwrap();
        assert_eq!(
            records.len(),
            1,
            "Note should still exist after skill deletion"
        );
    }
}
