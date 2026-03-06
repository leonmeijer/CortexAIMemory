//! Knowledge Notes CRUD operations for IndentiaGraphStore.
//!
//! Implements the full Notes lifecycle: CRUD, linking, propagation,
//! lifecycle management, embeddings, synapses, and energy.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::notes::{
    EntityType, Note, NoteAnchor, NoteFilters, NoteImportance, NoteScope, NoteStatus, NoteType,
    PropagatedNote, RelationHop,
};
use std::str::FromStr;
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

// ---------------------------------------------------------------------------
// Record types (module-level for SurrealValue derive)
// ---------------------------------------------------------------------------

#[derive(Debug, SurrealValue)]
pub(crate) struct NoteRecord {
    pub(crate) id: RecordId,
    pub(crate) project_id: Option<String>,
    pub(crate) note_type: String,
    pub(crate) status: String,
    pub(crate) importance: String,
    pub(crate) content: String,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) scope_type: Option<String>,
    pub(crate) scope_path: Option<String>,
    pub(crate) staleness_score: Option<f64>,
    pub(crate) energy: Option<f64>,
    pub(crate) code_anchor_hash: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: Option<String>,
    pub(crate) confirmed_at: Option<String>,
    // Can be either an array<number> (current schema) or legacy string payload.
    pub(crate) embedding: Option<serde_json::Value>,
    pub(crate) embedding_model: Option<String>,
    // Extended fields (added via ensure_note_schema_extensions)
    pub(crate) created_by: Option<String>,
    pub(crate) confirmed_by: Option<String>,
    pub(crate) last_activated: Option<String>,
    pub(crate) changes_json: Option<String>,
    pub(crate) assertion_rule_json: Option<String>,
    pub(crate) assertion_result_json: Option<String>,
}

#[derive(Debug, SurrealValue)]
struct SynapseRecord {
    out: RecordId,
    weight: f64,
}

#[derive(Debug, SurrealValue)]
struct CrossSynapseRecord {
    out: RecordId,
    weight: f64,
    entity_type: Option<String>,
}

#[derive(Debug, SurrealValue)]
struct SupersedesOutRecord {
    out: RecordId,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn parse_note_type(s: &str) -> NoteType {
    NoteType::from_str(s).unwrap_or(NoteType::Observation)
}

fn parse_note_status(s: &str) -> NoteStatus {
    NoteStatus::from_str(s).unwrap_or(NoteStatus::Active)
}

fn parse_note_importance(s: &str) -> NoteImportance {
    NoteImportance::from_str(s).unwrap_or(NoteImportance::Medium)
}

fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| s.parse::<DateTime<Utc>>().ok())
}

fn scope_from_parts(scope_type: Option<&str>, scope_path: Option<&str>) -> NoteScope {
    match scope_type {
        Some("workspace") => NoteScope::Workspace,
        Some("project") => NoteScope::Project,
        Some("module") => NoteScope::Module(scope_path.unwrap_or("").to_string()),
        Some("file") => NoteScope::File(scope_path.unwrap_or("").to_string()),
        Some("function") => NoteScope::Function(scope_path.unwrap_or("").to_string()),
        Some("struct") => NoteScope::Struct(scope_path.unwrap_or("").to_string()),
        Some("trait") => NoteScope::Trait(scope_path.unwrap_or("").to_string()),
        _ => NoteScope::Project,
    }
}

fn scope_to_parts(scope: &NoteScope) -> (String, Option<String>) {
    match scope {
        NoteScope::Workspace => ("workspace".to_string(), None),
        NoteScope::Project => ("project".to_string(), None),
        NoteScope::Module(p) => ("module".to_string(), Some(p.clone())),
        NoteScope::File(p) => ("file".to_string(), Some(p.clone())),
        NoteScope::Function(p) => ("function".to_string(), Some(p.clone())),
        NoteScope::Struct(p) => ("struct".to_string(), Some(p.clone())),
        NoteScope::Trait(p) => ("trait".to_string(), Some(p.clone())),
    }
}

fn entity_type_to_table(et: &EntityType) -> &'static str {
    match et {
        EntityType::Project => "project",
        EntityType::File => "file",
        EntityType::Module => "file",
        EntityType::Function => "function",
        EntityType::Struct => "struct",
        EntityType::Trait => "trait",
        EntityType::Enum => "enum",
        EntityType::Impl => "impl",
        EntityType::Task => "task",
        EntityType::Plan => "plan",
        EntityType::Step => "step",
        EntityType::Commit => "commit",
        EntityType::Decision => "decision",
        EntityType::Constraint => "constraint",
        EntityType::Milestone => "milestone",
        EntityType::Release => "release",
        EntityType::Workspace => "workspace",
        EntityType::WorkspaceMilestone => "workspace_milestone",
        EntityType::Resource => "resource",
        EntityType::Component => "component",
    }
}

fn f32_slice_to_f64(v: &[f32]) -> Vec<f64> {
    v.iter().map(|&x| x as f64).collect()
}

fn f64_vec_to_f32(v: &[f64]) -> Vec<f32> {
    v.iter().map(|&x| x as f32).collect()
}

impl NoteRecord {
    pub(crate) fn into_note(self) -> Result<Note> {
        let id = rid_to_uuid(&self.id)?;
        let project_id = self
            .project_id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok());
        let (scope_type_str, scope_path_str) =
            (self.scope_type.as_deref(), self.scope_path.as_deref());
        let scope = scope_from_parts(scope_type_str, scope_path_str);

        let created_at = parse_datetime(&self.created_at).unwrap_or_else(Utc::now);
        let last_confirmed_at = self.confirmed_at.as_deref().and_then(parse_datetime);
        let last_activated = self.last_activated.as_deref().and_then(parse_datetime);

        let changes = self
            .changes_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let assertion_rule = self
            .assertion_rule_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        let last_assertion_result = self
            .assertion_result_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());

        // supersedes/superseded_by are tracked via edges, not stored on the record.
        // They will be populated separately if needed.

        Ok(Note {
            id,
            project_id,
            note_type: parse_note_type(&self.note_type),
            status: parse_note_status(&self.status),
            importance: parse_note_importance(&self.importance),
            scope,
            content: self.content,
            tags: self.tags.unwrap_or_default(),
            anchors: vec![], // populated from attached_to edges if needed
            created_at,
            created_by: self.created_by.unwrap_or_else(|| "unknown".to_string()),
            last_confirmed_at,
            last_confirmed_by: self.confirmed_by,
            staleness_score: self.staleness_score.unwrap_or(0.0),
            energy: self.energy.unwrap_or(1.0),
            last_activated,
            supersedes: None,    // populated from edges
            superseded_by: None, // populated from edges
            changes,
            valid_at: None,
            invalid_at: None,
            assertion_rule,
            last_assertion_result,
        })
    }
}

// ---------------------------------------------------------------------------
// Schema extensions
// ---------------------------------------------------------------------------

impl IndentiaGraphStore {
    /// Ensure extra fields exist on the note table.
    ///
    /// The base schema defines the core fields. This adds extended fields
    /// that are needed for full Note round-tripping. Idempotent.
    pub(crate) async fn ensure_note_schema_extensions(&self) -> Result<()> {
        self.db
            .query(
                r#"
DEFINE FIELD IF NOT EXISTS created_by ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS confirmed_by ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS last_activated ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS changes_json ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS assertion_rule_json ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS assertion_result_json ON `note` TYPE option<string>;

DEFINE FIELD IF NOT EXISTS signature_hash ON `attached_to` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS body_hash ON `attached_to` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS entity_type ON `attached_to` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS entity_id ON `attached_to` TYPE option<string>;

DEFINE FIELD IF NOT EXISTS entity_type ON `synapse` TYPE option<string>;
"#,
            )
            .await
            .context("Failed to ensure note schema extensions")?;
        Ok(())
    }

    // =======================================================================
    // Core CRUD (5)
    // =======================================================================

    /// Create a note.
    pub async fn create_note(&self, note: &Note) -> Result<()> {
        self.ensure_note_schema_extensions().await?;
        let rid = RecordId::new("note", note.id.to_string().as_str());
        let (scope_type, scope_path) = scope_to_parts(&note.scope);
        let changes_json = if note.changes.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&note.changes).unwrap_or_default())
        };
        let assertion_rule_json = note
            .assertion_rule
            .as_ref()
            .map(|r| serde_json::to_string(r).unwrap_or_default());
        let assertion_result_json = note
            .last_assertion_result
            .as_ref()
            .map(|r| serde_json::to_string(r).unwrap_or_default());

        self.db
            .query(
                "CREATE $rid SET \
                 project_id = $pid, note_type = $nt, status = $st, \
                 importance = $imp, content = $con, tags = $tags, \
                 scope_type = $stype, scope_path = $spath, \
                 staleness_score = $ss, energy = $en, \
                 code_anchor_hash = $cah, \
                 created_at = $ca, updated_at = $ua, confirmed_at = $cfa, \
                 created_by = $cb, confirmed_by = $cfb, \
                 last_activated = $la, \
                 changes_json = $chg, assertion_rule_json = $arj, \
                 assertion_result_json = $arsj \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("pid", note.project_id.map(|u| u.to_string())))
            .bind(("nt", note.note_type.to_string()))
            .bind(("st", note.status.to_string()))
            .bind(("imp", note.importance.to_string()))
            .bind(("con", note.content.clone()))
            .bind((
                "tags",
                if note.tags.is_empty() {
                    None
                } else {
                    Some(note.tags.clone())
                },
            ))
            .bind(("stype", Some(scope_type)))
            .bind(("spath", scope_path))
            .bind(("ss", Some(note.staleness_score)))
            .bind(("en", Some(note.energy)))
            .bind(("cah", Option::<String>::None))
            .bind(("ca", note.created_at.to_rfc3339()))
            .bind(("ua", Option::<String>::None))
            .bind(("cfa", note.last_confirmed_at.map(|d| d.to_rfc3339())))
            .bind(("cb", Some(note.created_by.clone())))
            .bind(("cfb", note.last_confirmed_by.clone()))
            .bind(("la", note.last_activated.map(|d| d.to_rfc3339())))
            .bind(("chg", changes_json))
            .bind(("arj", assertion_rule_json))
            .bind(("arsj", assertion_result_json))
            .await
            .context("Failed to create note")?;

        // Create attached_to edges for anchors
        for anchor in &note.anchors {
            let table = entity_type_to_table(&anchor.entity_type);
            let note_rid = RecordId::new("note", note.id.to_string().as_str());
            let entity_rid = RecordId::new(table, anchor.entity_id.as_str());
            self.db
                .query(
                    "RELATE $from->attached_to->$to SET \
                     anchor_type = $at, signature_hash = $sh, body_hash = $bh, \
                     entity_type = $et, entity_id = $eid \
                     RETURN NONE",
                )
                .bind(("from", note_rid))
                .bind(("to", entity_rid))
                .bind(("at", anchor.entity_type.to_string()))
                .bind(("sh", anchor.signature_hash.clone()))
                .bind(("bh", anchor.body_hash.clone()))
                .bind(("et", anchor.entity_type.to_string()))
                .bind(("eid", anchor.entity_id.clone()))
                .await
                .context("Failed to create attached_to edge")?;
        }

        // Create supersedes edge if applicable
        if let Some(supersedes_id) = note.supersedes {
            let new_rid = RecordId::new("note", note.id.to_string().as_str());
            let old_rid = RecordId::new("note", supersedes_id.to_string().as_str());
            self.db
                .query("RELATE $from->supersedes->$to RETURN NONE")
                .bind(("from", new_rid))
                .bind(("to", old_rid))
                .await
                .context("Failed to create supersedes edge")?;
        }

        Ok(())
    }

    /// Get a note by ID.
    pub async fn get_note(&self, id: Uuid) -> Result<Option<Note>> {
        self.ensure_note_schema_extensions().await?;
        let rid = RecordId::new("note", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get note")?;
        let records: Vec<NoteRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => {
                let mut note = r.into_note()?;
                // Populate supersedes/superseded_by from edges
                self.populate_note_succession(&mut note).await?;
                Ok(Some(note))
            }
            None => Ok(None),
        }
    }

    /// Update a note (partial update). Returns the updated note if found.
    pub async fn update_note(
        &self,
        id: Uuid,
        content: Option<String>,
        importance: Option<NoteImportance>,
        status: Option<NoteStatus>,
        tags: Option<Vec<String>>,
        staleness_score: Option<f64>,
    ) -> Result<Option<Note>> {
        self.ensure_note_schema_extensions().await?;
        let mut sets = Vec::new();
        if content.is_some() {
            sets.push("content = $con");
        }
        if importance.is_some() {
            sets.push("importance = $imp");
        }
        if status.is_some() {
            sets.push("status = $st");
        }
        if tags.is_some() {
            sets.push("tags = $tags");
        }
        if staleness_score.is_some() {
            sets.push("staleness_score = $ss");
        }
        sets.push("updated_at = $ua");
        if sets.is_empty() {
            return self.get_note(id).await;
        }

        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let rid = RecordId::new("note", id.to_string().as_str());
        let mut q = self.db.query(&query);
        q = q.bind(("rid", rid));
        q = q.bind(("ua", Utc::now().to_rfc3339()));
        if let Some(ref c) = content {
            q = q.bind(("con", c.clone()));
        }
        if let Some(ref i) = importance {
            q = q.bind(("imp", i.to_string()));
        }
        if let Some(ref s) = status {
            q = q.bind(("st", s.to_string()));
        }
        if let Some(ref t) = tags {
            q = q.bind(("tags", t.clone()));
        }
        if let Some(ss) = staleness_score {
            q = q.bind(("ss", ss));
        }

        q.await.context("Failed to update note")?;
        self.get_note(id).await
    }

    /// Delete a note and all associated edges.
    pub async fn delete_note(&self, id: Uuid) -> Result<bool> {
        self.ensure_note_schema_extensions().await?;
        let did = id.to_string();
        let rid = RecordId::new("note", did.as_str());

        // Check existence first
        let exists = self.get_note(id).await?.is_some();
        if !exists {
            return Ok(false);
        }

        self.db
            .query(
                "DELETE FROM attached_to WHERE in = type::record('note', $did);\
                 DELETE FROM supersedes WHERE in = type::record('note', $did) OR out = type::record('note', $did);\
                 DELETE FROM synapse WHERE in = type::record('note', $did) OR out = type::record('note', $did);\
                 DELETE $rid",
            )
            .bind(("did", did))
            .bind(("rid", rid))
            .await
            .context("Failed to delete note")?;
        Ok(true)
    }

    /// List notes with filters and pagination.
    pub async fn list_notes(
        &self,
        project_id: Option<Uuid>,
        _workspace_slug: Option<&str>,
        filters: &NoteFilters,
    ) -> Result<(Vec<Note>, usize)> {
        self.ensure_note_schema_extensions().await?;

        // Build WHERE conditions. Scalar user-supplied values (scope_type, search)
        // are referenced via $-placeholders to prevent injection. Enum-derived
        // values (status, note_type, importance) are validated by their `to_string`
        // implementations and embedded as literals inside SurrealQL arrays.
        let mut conditions = Vec::new();

        if project_id.is_some() {
            conditions.push("project_id = $filter_pid".to_string());
        }
        if let Some(true) = filters.global_only {
            conditions.push("project_id = NONE".to_string());
        }
        if let Some(ref statuses) = filters.status {
            let vals: Vec<String> = statuses.iter().map(|s| format!("'{}'", s)).collect();
            conditions.push(format!("status IN [{}]", vals.join(",")));
        }
        if let Some(ref note_types) = filters.note_type {
            let vals: Vec<String> = note_types.iter().map(|t| format!("'{}'", t)).collect();
            conditions.push(format!("note_type IN [{}]", vals.join(",")));
        }
        if let Some(ref importances) = filters.importance {
            let vals: Vec<String> = importances.iter().map(|i| format!("'{}'", i)).collect();
            conditions.push(format!("importance IN [{}]", vals.join(",")));
        }
        if filters.scope_type.is_some() {
            conditions.push("scope_type = $filter_scope_type".to_string());
        }
        if filters.search.is_some() {
            conditions.push("string::lowercase(content) CONTAINS $filter_search".to_string());
        }
        if let Some(min_s) = filters.min_staleness {
            conditions.push(format!("staleness_score >= {}", min_s));
        }
        if let Some(max_s) = filters.max_staleness {
            conditions.push(format!("staleness_score <= {}", max_s));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Validate sort_field to a known allowlist to prevent injection.
        let sort_field = match filters.sort_by.as_deref().unwrap_or("created_at") {
            "created_at" | "updated_at" | "staleness_score" | "importance" | "status"
            | "note_type" | "energy" => filters.sort_by.as_deref().unwrap_or("created_at"),
            _ => "created_at",
        };
        let sort_order = match filters.sort_order.as_deref().unwrap_or("DESC") {
            "asc" | "ASC" => "ASC",
            _ => "DESC",
        };
        let limit = filters.limit.unwrap_or(50) as usize;
        let offset = filters.offset.unwrap_or(0) as usize;

        let count_query = format!(
            "SELECT count() AS total FROM note {} GROUP ALL",
            where_clause
        );
        let data_query = format!(
            "SELECT * FROM note {} ORDER BY {} {} LIMIT {} START {}",
            where_clause, sort_field, sort_order, limit, offset
        );
        let combined = format!("{}; {}", count_query, data_query);

        let mut qb = self.db.query(&combined);
        if let Some(pid) = project_id {
            qb = qb.bind(("filter_pid", pid.to_string()));
        }
        if let Some(ref st) = filters.scope_type {
            qb = qb.bind(("filter_scope_type", st.clone()));
        }
        if let Some(ref search) = filters.search {
            qb = qb.bind(("filter_search", search.to_lowercase()));
        }

        let mut resp = qb.await.context("Failed to list notes")?;

        let count_result: Vec<serde_json::Value> = resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let records: Vec<NoteRecord> = resp.take(1)?;
        let notes: Result<Vec<Note>> = records.into_iter().map(|r| r.into_note()).collect();
        Ok((notes?, total))
    }

    // =======================================================================
    // Linking (3)
    // =======================================================================

    /// Link a note to an entity via attached_to edge.
    pub async fn link_note_to_entity(
        &self,
        note_id: Uuid,
        entity_type: &EntityType,
        entity_id: &str,
        signature_hash: Option<&str>,
        body_hash: Option<&str>,
    ) -> Result<()> {
        self.ensure_note_schema_extensions().await?;
        let table = entity_type_to_table(entity_type);
        let note_rid = RecordId::new("note", note_id.to_string().as_str());
        let entity_rid = RecordId::new(table, entity_id);
        self.db
            .query(
                "RELATE $from->attached_to->$to SET \
                 anchor_type = $at, signature_hash = $sh, body_hash = $bh, \
                 entity_type = $et, entity_id = $eid \
                 RETURN NONE",
            )
            .bind(("from", note_rid))
            .bind(("to", entity_rid))
            .bind(("at", entity_type.to_string()))
            .bind(("sh", signature_hash.map(|s| s.to_string())))
            .bind(("bh", body_hash.map(|s| s.to_string())))
            .bind(("et", entity_type.to_string()))
            .bind(("eid", entity_id.to_string()))
            .await
            .context("Failed to link note to entity")?;
        Ok(())
    }

    /// Unlink a note from an entity.
    pub async fn unlink_note_from_entity(
        &self,
        note_id: Uuid,
        entity_type: &EntityType,
        entity_id: &str,
    ) -> Result<()> {
        let note_rid = RecordId::new("note", note_id.to_string().as_str());
        let table = entity_type_to_table(entity_type);
        let entity_rid = RecordId::new(table, entity_id);
        self.db
            .query(
                "DELETE FROM attached_to WHERE \
                 in = $note_rid AND out = $entity_rid",
            )
            .bind(("note_rid", note_rid))
            .bind(("entity_rid", entity_rid))
            .await
            .context("Failed to unlink note from entity")?;
        Ok(())
    }

    /// Get all notes linked to an entity.
    pub async fn get_notes_for_entity(
        &self,
        entity_type: &EntityType,
        entity_id: &str,
    ) -> Result<Vec<Note>> {
        self.ensure_note_schema_extensions().await?;
        let table = entity_type_to_table(entity_type);
        let entity_rid = RecordId::new(table, entity_id);
        let mut resp = self
            .db
            .query(
                "SELECT * FROM note WHERE id IN \
                 (SELECT VALUE in.id FROM attached_to WHERE out = $entity_rid)",
            )
            .bind(("entity_rid", entity_rid))
            .await
            .context("Failed to get notes for entity")?;
        let records: Vec<NoteRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_note()).collect()
    }

    // =======================================================================
    // Propagation (2)
    // =======================================================================

    /// Get propagated notes for an entity by traversing the graph.
    ///
    /// Walks outgoing relationships (IMPORTS, CALLS, CONTAINS, etc.) from the
    /// target entity up to `max_depth` hops. Collects notes attached to each
    /// intermediate node and scores them by distance and importance.
    pub async fn get_propagated_notes(
        &self,
        entity_type: &EntityType,
        entity_id: &str,
        max_depth: u32,
        min_score: f64,
        _relation_types: Option<&[String]>,
    ) -> Result<Vec<PropagatedNote>> {
        self.ensure_note_schema_extensions().await?;
        // Direct notes (distance 0)
        let direct_notes = self.get_notes_for_entity(entity_type, entity_id).await?;
        let mut results: Vec<PropagatedNote> = Vec::new();

        for note in direct_notes {
            let score = note.importance.weight();
            if score >= min_score {
                results.push(PropagatedNote {
                    note,
                    relevance_score: score,
                    source_entity: entity_id.to_string(),
                    propagation_path: vec![entity_id.to_string()],
                    distance: 0,
                    path_pagerank: None,
                    relation_path: vec![],
                    path_rel_weight: None,
                });
            }
        }

        // For depths 1+, walk IMPORTS and CALLS edges
        if max_depth > 0 {
            let table = entity_type_to_table(entity_type);
            // Walk outgoing edges (entity imports/calls -> neighbor has notes)
            for depth in 1..=max_depth {
                let entity_rid = RecordId::new(table, entity_id);
                let mut resp = self
                    .db
                    .query(
                        "SELECT * FROM note WHERE id IN \
                         (SELECT VALUE in.id FROM attached_to WHERE out IN \
                           (SELECT VALUE out.id FROM imports WHERE in = $entity_rid))",
                    )
                    .bind(("entity_rid", entity_rid))
                    .await
                    .context("Failed to get propagated notes")?;
                let records: Vec<NoteRecord> = resp.take(0)?;
                for record in records {
                    let note = record.into_note()?;
                    let score = (1.0 / (depth as f64 + 1.0)) * note.importance.weight();
                    if score >= min_score {
                        results.push(PropagatedNote {
                            relevance_score: score,
                            source_entity: entity_id.to_string(),
                            propagation_path: vec![entity_id.to_string()],
                            distance: depth,
                            path_pagerank: None,
                            relation_path: vec![RelationHop::structural("IMPORTS")],
                            path_rel_weight: None,
                            note,
                        });
                    }
                }
                // Only one depth step via SQL for now
                break;
            }
        }

        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    /// Get workspace-level notes that propagate to a project.
    pub async fn get_workspace_notes_for_project(
        &self,
        project_id: Uuid,
        propagation_factor: f64,
    ) -> Result<Vec<PropagatedNote>> {
        self.ensure_note_schema_extensions().await?;
        // Find workspace(s) this project belongs to
        let project_rid = RecordId::new("project", project_id.to_string().as_str());
        let mut resp = self
            .db
            .query(
                "SELECT VALUE out.id FROM belongs_to_workspace \
                 WHERE in = $project_rid",
            )
            .bind(("project_rid", project_rid))
            .await
            .context("Failed to find workspaces for project")?;
        let workspace_ids: Vec<serde_json::Value> = resp.take(0)?;

        let pid = project_id.to_string();
        let mut results = Vec::new();
        for ws_val in workspace_ids {
            // Extract workspace id string
            let ws_id = match ws_val {
                serde_json::Value::String(s) => s,
                _ => continue,
            };

            // Get notes attached to this workspace.
            // ws_id comes from the database (not user input), but we still use
            // a parameterized query for consistency.
            let workspace_rid = RecordId::new("workspace", ws_id.as_str());
            let mut ws_resp = self
                .db
                .query(
                    "SELECT * FROM note WHERE id IN \
                     (SELECT VALUE in.id FROM attached_to WHERE out = $workspace_rid)",
                )
                .bind(("workspace_rid", workspace_rid))
                .await
                .context("Failed to get workspace notes")?;
            let records: Vec<NoteRecord> = ws_resp.take(0)?;
            for record in records {
                let note = record.into_note()?;
                let score = note.importance.weight() * propagation_factor;
                results.push(PropagatedNote {
                    relevance_score: score,
                    source_entity: format!("workspace:{}", ws_id),
                    propagation_path: vec![format!("workspace:{}", ws_id), pid.clone()],
                    distance: 1,
                    path_pagerank: None,
                    relation_path: vec![RelationHop::structural("BELONGS_TO_WORKSPACE")],
                    path_rel_weight: Some(propagation_factor),
                    note,
                });
            }
        }

        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    // =======================================================================
    // Lifecycle (4)
    // =======================================================================

    /// Supersede one note with another.
    pub async fn supersede_note(&self, old_note_id: Uuid, new_note_id: Uuid) -> Result<()> {
        self.ensure_note_schema_extensions().await?;
        let new_rid = RecordId::new("note", new_note_id.to_string().as_str());
        let old_rid = RecordId::new("note", old_note_id.to_string().as_str());
        self.db
            .query(
                "RELATE $from->supersedes->$to RETURN NONE;\
                 UPDATE $old SET status = 'obsolete' RETURN NONE",
            )
            .bind(("from", new_rid))
            .bind(("to", old_rid.clone()))
            .bind(("old", old_rid))
            .await
            .context("Failed to supersede note")?;
        Ok(())
    }

    /// Confirm a note is still valid.
    pub async fn confirm_note(&self, note_id: Uuid, confirmed_by: &str) -> Result<Option<Note>> {
        self.ensure_note_schema_extensions().await?;
        let now = Utc::now().to_rfc3339();
        let rid = RecordId::new("note", note_id.to_string().as_str());
        self.db
            .query(
                "UPDATE $rid SET \
                 confirmed_at = $now, confirmed_by = $cb, \
                 staleness_score = 0.0, status = 'active', \
                 last_activated = $now \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("now", now))
            .bind(("cb", confirmed_by.to_string()))
            .await
            .context("Failed to confirm note")?;
        self.get_note(note_id).await
    }

    /// Get notes that need review (stale or needs_review status).
    pub async fn get_notes_needing_review(&self, project_id: Option<Uuid>) -> Result<Vec<Note>> {
        self.ensure_note_schema_extensions().await?;
        let sql = if project_id.is_some() {
            "SELECT * FROM note \
             WHERE (status = 'needs_review' OR status = 'stale') \
               AND project_id = $pid \
             ORDER BY staleness_score DESC"
        } else {
            "SELECT * FROM note \
             WHERE (status = 'needs_review' OR status = 'stale') \
             ORDER BY staleness_score DESC"
        };
        let mut qb = self.db.query(sql);
        if let Some(pid) = project_id {
            qb = qb.bind(("pid", pid.to_string()));
        }
        let mut resp = qb.await.context("Failed to get notes needing review")?;
        let records: Vec<NoteRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_note()).collect()
    }

    /// Update staleness scores for all active notes.
    /// Returns the number of notes updated.
    pub async fn update_staleness_scores(&self) -> Result<usize> {
        self.ensure_note_schema_extensions().await?;
        // Get all active notes
        let mut resp = self
            .db
            .query("SELECT * FROM note WHERE status = 'active'")
            .await
            .context("Failed to fetch active notes")?;
        let records: Vec<NoteRecord> = resp.take(0)?;

        let now = Utc::now();
        let mut updated = 0usize;

        for record in records {
            let note = record.into_note()?;
            let confirmed = note.last_confirmed_at.unwrap_or(note.created_at);
            let days_since = (now - confirmed).num_seconds() as f64 / 86400.0;
            let decay_days = note.base_decay_days() * note.importance.decay_factor();
            let staleness = (days_since / decay_days).min(1.0).max(0.0);

            let nid = note.id.to_string();
            let rid = RecordId::new("note", nid.as_str());

            // Determine if status should change
            let new_status = if staleness > 0.8 {
                "stale"
            } else if staleness > 0.5 {
                "needs_review"
            } else {
                "active"
            };

            self.db
                .query("UPDATE $rid SET staleness_score = $ss, status = $st RETURN NONE")
                .bind(("rid", rid))
                .bind(("ss", staleness))
                .bind(("st", new_status.to_string()))
                .await
                .context("Failed to update staleness score")?;
            updated += 1;
        }

        Ok(updated)
    }

    // =======================================================================
    // Anchors (1)
    // =======================================================================

    /// Get all anchors for a note.
    pub async fn get_note_anchors(&self, note_id: Uuid) -> Result<Vec<NoteAnchor>> {
        self.ensure_note_schema_extensions().await?;
        // Select only string properties to avoid SurrealDB record type deserialization
        let note_rid = RecordId::new("note", note_id.to_string().as_str());
        let mut resp = self
            .db
            .query(
                "SELECT entity_type, entity_id, signature_hash, body_hash, anchor_type \
                 FROM attached_to WHERE in = $note_rid",
            )
            .bind(("note_rid", note_rid))
            .await
            .context("Failed to get note anchors")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;

        let mut anchors = Vec::new();
        for record in records {
            let entity_type_str = record
                .get("entity_type")
                .and_then(|v| v.as_str())
                .unwrap_or("file");
            let entity_id = record
                .get("entity_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let sig_hash = record
                .get("signature_hash")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let body_hash = record
                .get("body_hash")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if let Ok(et) = EntityType::from_str(entity_type_str) {
                anchors.push(NoteAnchor {
                    entity_type: et,
                    entity_id: entity_id.to_string(),
                    signature_hash: sig_hash,
                    body_hash,
                    last_verified: Utc::now(),
                    is_valid: true,
                });
            }
        }
        Ok(anchors)
    }

    // =======================================================================
    // Embeddings (4)
    // =======================================================================

    /// Set the vector embedding for a note.
    ///
    /// Note: the `note` schema defines `embedding` as `option<array>`, so we
    /// bind the embedding as a native array (not a JSON string).
    pub async fn set_note_embedding(
        &self,
        note_id: Uuid,
        embedding: &[f32],
        model: &str,
    ) -> Result<()> {
        let emb_f64 = f32_slice_to_f64(embedding);
        let rid = RecordId::new("note", note_id.to_string().as_str());
        self.db
            .query("UPDATE $rid SET embedding = $emb, embedding_model = $model RETURN NONE")
            .bind(("rid", rid))
            .bind(("emb", emb_f64))
            .bind(("model", model.to_string()))
            .await
            .context("Failed to set note embedding")?;
        Ok(())
    }

    /// Get the vector embedding for a note.
    pub async fn get_note_embedding(&self, note_id: Uuid) -> Result<Option<Vec<f32>>> {
        let rid = RecordId::new("note", note_id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT embedding FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get note embedding")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        if let Some(record) = records.first() {
            // embedding is stored as a native array, so it comes back as a JSON array
            if let Some(emb_arr) = record.get("embedding").and_then(|v| v.as_array()) {
                let emb_f64: Vec<f64> = emb_arr.iter().filter_map(|v| v.as_f64()).collect();
                if emb_f64.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(f64_vec_to_f32(&emb_f64)));
            }
            // Fallback: try as string (for backwards compatibility)
            if let Some(emb_str) = record.get("embedding").and_then(|v| v.as_str()) {
                let emb_f64: Vec<f64> = serde_json::from_str(emb_str).unwrap_or_default();
                if emb_f64.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(f64_vec_to_f32(&emb_f64)));
            }
        }
        Ok(None)
    }

    /// Search notes by vector similarity (in-memory cosine fallback).
    ///
    /// Since embedding is stored as `option<array>` in the schema (not a string),
    /// we use serde_json::Value for deserialization to extract the embedding array
    /// alongside the note fields.
    pub async fn vector_search_notes(
        &self,
        embedding: &[f32],
        limit: usize,
        project_id: Option<Uuid>,
        _workspace_slug: Option<&str>,
        min_similarity: Option<f64>,
    ) -> Result<Vec<(Note, f64)>> {
        self.ensure_note_schema_extensions().await?;
        // Select specific fields to avoid record-type deserialization issues.
        // Use meta::id(id) to get the raw string key instead of a Record type.
        let sql = if project_id.is_some() {
            "SELECT meta::id(id) AS uid, project_id, note_type, status, \
             importance, content, tags, scope_type, scope_path, \
             staleness_score, energy, created_at, created_by, \
             confirmed_at, confirmed_by, last_activated, \
             changes_json, assertion_rule_json, assertion_result_json, \
             embedding \
             FROM note WHERE embedding != NONE AND project_id = $pid"
        } else {
            "SELECT meta::id(id) AS uid, project_id, note_type, status, \
             importance, content, tags, scope_type, scope_path, \
             staleness_score, energy, created_at, created_by, \
             confirmed_at, confirmed_by, last_activated, \
             changes_json, assertion_rule_json, assertion_result_json, \
             embedding \
             FROM note WHERE embedding != NONE"
        };
        let mut qb = self.db.query(sql);
        if let Some(pid) = project_id {
            qb = qb.bind(("pid", pid.to_string()));
        }
        let mut resp = qb.await.context("Failed to search notes by vector")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;

        let query_f64: Vec<f64> = embedding.iter().map(|&x| x as f64).collect();
        let min_sim = min_similarity.unwrap_or(0.0);
        let mut scored: Vec<(Note, f64)> = Vec::new();

        for record in records {
            // Extract embedding as native array
            let emb_opt: Option<Vec<f64>> = record
                .get("embedding")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect());
            if let Some(ref emb) = emb_opt {
                if emb.is_empty() {
                    continue;
                }
                let sim = cosine_similarity(&query_f64, emb);
                if sim >= min_sim {
                    // Reconstruct note from JSON value
                    let note = json_value_to_note(&record)?;
                    scored.push((note, sim));
                }
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    /// List notes that don't have embeddings yet.
    pub async fn list_notes_without_embedding(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<Note>, usize)> {
        self.ensure_note_schema_extensions().await?;
        // Count
        let mut count_resp = self
            .db
            .query(
                "SELECT count() AS total FROM note \
                 WHERE embedding = NONE \
                 GROUP ALL",
            )
            .await
            .context("Failed to count notes without embedding")?;
        let count_result: Vec<serde_json::Value> = count_resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let mut resp = self
            .db
            .query(
                "SELECT * FROM note \
                 WHERE embedding = NONE \
                 ORDER BY created_at ASC LIMIT $limit START $offset",
            )
            .bind(("limit", limit as i64))
            .bind(("offset", offset as i64))
            .await
            .context("Failed to list notes without embedding")?;
        let records: Vec<NoteRecord> = resp.take(0)?;
        let notes: Result<Vec<Note>> = records.into_iter().map(|r| r.into_note()).collect();
        Ok((notes?, total))
    }

    // =======================================================================
    // Synapses (7)
    // =======================================================================

    /// Create synapse edges from a note to its neighbors.
    pub async fn create_synapses(&self, note_id: Uuid, neighbors: &[(Uuid, f64)]) -> Result<usize> {
        let mut created = 0usize;
        let nid = note_id.to_string();
        for (neighbor_id, weight) in neighbors {
            let from_rid = RecordId::new("note", nid.as_str());
            let to_rid = RecordId::new("note", neighbor_id.to_string().as_str());
            self.db
                .query("RELATE $from->synapse->$to SET weight = $w RETURN NONE")
                .bind(("from", from_rid))
                .bind(("to", to_rid))
                .bind(("w", *weight))
                .await
                .context("Failed to create synapse")?;
            created += 1;
        }
        Ok(created)
    }

    /// Get all synapses for a note (outgoing).
    pub async fn get_synapses(&self, note_id: Uuid) -> Result<Vec<(Uuid, f64)>> {
        let note_rid = RecordId::new("note", note_id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT out, weight FROM synapse WHERE in = $note_rid")
            .bind(("note_rid", note_rid))
            .await
            .context("Failed to get synapses")?;
        let records: Vec<SynapseRecord> = resp.take(0)?;
        let mut result = Vec::new();
        for r in records {
            if let Ok(uid) = rid_to_uuid(&r.out) {
                result.push((uid, r.weight));
            }
        }
        Ok(result)
    }

    /// Delete all synapses for a note.
    pub async fn delete_synapses(&self, note_id: Uuid) -> Result<usize> {
        // Count existing
        let synapses = self.get_synapses(note_id).await?;
        let count = synapses.len();

        let note_rid = RecordId::new("note", note_id.to_string().as_str());
        self.db
            .query("DELETE FROM synapse WHERE in = $note_rid")
            .bind(("note_rid", note_rid))
            .await
            .context("Failed to delete synapses")?;
        Ok(count)
    }

    /// List notes that have embeddings but no synapse edges.
    pub async fn list_notes_needing_synapses(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<Note>, usize)> {
        self.ensure_note_schema_extensions().await?;
        // Count
        let mut count_resp = self
            .db
            .query(
                "SELECT count() AS total FROM note \
                 WHERE embedding != NONE \
                 AND id NOT IN (SELECT VALUE in.id FROM synapse WHERE in.id IS NOT NONE) \
                 GROUP ALL",
            )
            .await
            .context("Failed to count notes needing synapses")?;
        let count_result: Vec<serde_json::Value> = count_resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let mut resp = self
            .db
            .query(
                "SELECT * FROM note \
                 WHERE embedding != NONE \
                 AND id NOT IN (SELECT VALUE in.id FROM synapse WHERE in.id IS NOT NONE) \
                 ORDER BY created_at ASC LIMIT $limit START $offset",
            )
            .bind(("limit", limit as i64))
            .bind(("offset", offset as i64))
            .await
            .context("Failed to list notes needing synapses")?;
        let records: Vec<NoteRecord> = resp.take(0)?;
        let notes: Result<Vec<Note>> = records.into_iter().map(|r| r.into_note()).collect();
        Ok((notes?, total))
    }

    /// Create cross-entity synapse edges (note->decision or decision->note).
    pub async fn create_cross_entity_synapses(
        &self,
        source_id: Uuid,
        neighbors: &[(Uuid, f64)],
    ) -> Result<usize> {
        self.ensure_note_schema_extensions().await?;
        let mut created = 0usize;
        let sid = source_id.to_string();
        for (neighbor_id, weight) in neighbors {
            // Try note->note first, then note->decision
            let from_rid = RecordId::new("note", sid.as_str());
            let to_rid = RecordId::new("note", neighbor_id.to_string().as_str());
            self.db
                .query(
                    "RELATE $from->synapse->$to SET weight = $w, entity_type = 'cross' RETURN NONE",
                )
                .bind(("from", from_rid))
                .bind(("to", to_rid))
                .bind(("w", *weight))
                .await
                .context("Failed to create cross-entity synapse")?;
            created += 1;
        }
        Ok(created)
    }

    /// Get cross-entity synapses for a node.
    pub async fn get_cross_entity_synapses(
        &self,
        node_id: Uuid,
    ) -> Result<Vec<(Uuid, f64, String)>> {
        let note_rid = RecordId::new("note", node_id.to_string().as_str());
        let mut resp = self
            .db
            .query(
                "SELECT out, weight, entity_type FROM synapse \
                 WHERE in = $note_rid AND entity_type = 'cross'",
            )
            .bind(("note_rid", note_rid))
            .await
            .context("Failed to get cross-entity synapses")?;
        let records: Vec<CrossSynapseRecord> = resp.take(0)?;
        let mut result = Vec::new();
        for r in records {
            if let Ok(uid) = rid_to_uuid(&r.out) {
                result.push((uid, r.weight, r.entity_type.unwrap_or_default()));
            }
        }
        Ok(result)
    }

    /// Get all synapse edges for a project.
    pub async fn get_project_synapse_edges(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<(String, String, f64)>> {
        // Get note IDs belonging to this project
        let mut resp = self
            .db
            .query("SELECT id FROM note WHERE project_id = $pid")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to get project notes for synapses")?;
        let note_ids: Vec<serde_json::Value> = resp.take(0)?;

        let mut edges = Vec::new();
        for note_val in &note_ids {
            let note_id_str = note_val.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if note_id_str.is_empty() {
                continue;
            }

            // Extract UUID from "note:⟨uuid⟩" format
            let source_trimmed = note_id_str
                .trim_start_matches("note:")
                .trim_start_matches('⟨')
                .trim_end_matches('⟩');

            // Use a parameterized query with the RecordId to avoid raw interpolation.
            let note_rid = RecordId::new("note", source_trimmed);
            if let Ok(mut syn_resp) = self
                .db
                .query("SELECT out, weight FROM synapse WHERE in = $note_rid")
                .bind(("note_rid", note_rid))
                .await
            {
                let syn_records: Vec<SynapseRecord> = syn_resp.take(0).unwrap_or_default();
                for syn in syn_records {
                    if let Ok(target_uuid) = rid_to_uuid(&syn.out) {
                        edges.push((
                            source_trimmed.to_string(),
                            target_uuid.to_string(),
                            syn.weight,
                        ));
                    }
                }
            }
        }
        Ok(edges)
    }

    // =======================================================================
    // Energy (5)
    // =======================================================================

    /// Update energy scores for all notes using exponential decay.
    pub async fn update_energy_scores(&self, half_life_days: f64) -> Result<usize> {
        self.ensure_note_schema_extensions().await?;
        let mut resp = self
            .db
            .query("SELECT * FROM note WHERE energy != NONE")
            .await
            .context("Failed to fetch notes for energy update")?;
        let records: Vec<NoteRecord> = resp.take(0)?;

        let now = Utc::now();
        let decay_constant = (0.5_f64).ln() / half_life_days;
        let mut updated = 0usize;

        for record in records {
            let note = record.into_note()?;
            let last_active = note.last_activated.unwrap_or(note.created_at);
            let days_idle = (now - last_active).num_seconds() as f64 / 86400.0;
            let new_energy = (note.energy * (decay_constant * days_idle).exp()).clamp(0.0, 1.0);

            let nid = note.id.to_string();
            let rid = RecordId::new("note", nid.as_str());
            self.db
                .query("UPDATE $rid SET energy = $en RETURN NONE")
                .bind(("rid", rid))
                .bind(("en", new_energy))
                .await
                .context("Failed to update energy score")?;
            updated += 1;
        }

        Ok(updated)
    }

    /// Boost the energy of a note.
    pub async fn boost_energy(&self, note_id: Uuid, amount: f64) -> Result<()> {
        self.ensure_note_schema_extensions().await?;
        let rid = RecordId::new("note", note_id.to_string().as_str());
        let now = Utc::now().to_rfc3339();
        // Fetch current energy
        let current = self.get_note(note_id).await?;
        let current_energy = current.map(|n| n.energy).unwrap_or(0.5);
        let new_energy = (current_energy + amount).min(1.0);

        self.db
            .query("UPDATE $rid SET energy = $en, last_activated = $la RETURN NONE")
            .bind(("rid", rid))
            .bind(("en", new_energy))
            .bind(("la", now))
            .await
            .context("Failed to boost energy")?;
        Ok(())
    }

    /// Reinforce synapses between co-activated notes.
    pub async fn reinforce_synapses(&self, note_ids: &[Uuid], boost: f64) -> Result<usize> {
        let mut reinforced = 0usize;
        for i in 0..note_ids.len() {
            for j in (i + 1)..note_ids.len() {
                let rid_a = RecordId::new("note", note_ids[i].to_string().as_str());
                let rid_b = RecordId::new("note", note_ids[j].to_string().as_str());
                // Try to update existing synapse a->b
                let mut resp = self
                    .db
                    .query(
                        "UPDATE synapse SET weight = weight + $boost \
                         WHERE in = $from AND out = $to",
                    )
                    .bind(("boost", boost))
                    .bind(("from", rid_a.clone()))
                    .bind(("to", rid_b.clone()))
                    .await
                    .context("Failed to reinforce synapse")?;
                let updated: Vec<serde_json::Value> = resp.take(0).unwrap_or_default();
                if !updated.is_empty() {
                    reinforced += 1;
                }
                // Also try b->a
                let mut resp2 = self
                    .db
                    .query(
                        "UPDATE synapse SET weight = weight + $boost \
                         WHERE in = $from AND out = $to",
                    )
                    .bind(("boost", boost))
                    .bind(("from", rid_b))
                    .bind(("to", rid_a))
                    .await
                    .context("Failed to reinforce synapse")?;
                let updated2: Vec<serde_json::Value> = resp2.take(0).unwrap_or_default();
                if !updated2.is_empty() {
                    reinforced += 1;
                }
            }
        }
        Ok(reinforced)
    }

    /// Decay all synapse weights and prune weak ones.
    /// Returns (decayed_count, pruned_count).
    pub async fn decay_synapses(
        &self,
        decay_amount: f64,
        prune_threshold: f64,
    ) -> Result<(usize, usize)> {
        // Decay all synapse weights
        let mut resp = self
            .db
            .query("UPDATE synapse SET weight = weight - $decay RETURN AFTER")
            .bind(("decay", decay_amount))
            .await
            .context("Failed to decay synapses")?;
        let decayed: Vec<serde_json::Value> = resp.take(0).unwrap_or_default();
        let decayed_count = decayed.len();

        // Prune synapses below threshold
        self.db
            .query("DELETE FROM synapse WHERE weight < $threshold")
            .bind(("threshold", prune_threshold))
            .await
            .context("Failed to prune synapses")?;

        // Count how many were pruned (those that went below threshold)
        let pruned_count = decayed
            .iter()
            .filter(|v| {
                v.get("weight")
                    .and_then(|w| w.as_f64())
                    .map(|w| w < prune_threshold)
                    .unwrap_or(false)
            })
            .count();

        Ok((decayed_count, pruned_count))
    }

    /// Initialize energy for notes that don't have it set.
    pub async fn init_note_energy(&self) -> Result<usize> {
        self.ensure_note_schema_extensions().await?;
        let mut resp = self
            .db
            .query("SELECT * FROM note WHERE energy = NONE")
            .await
            .context("Failed to fetch notes without energy")?;
        let records: Vec<NoteRecord> = resp.take(0)?;
        let count = records.len();

        if count > 0 {
            self.db
                .query("UPDATE note SET energy = 1.0 WHERE energy = NONE RETURN NONE")
                .await
                .context("Failed to init note energy")?;
        }

        Ok(count)
    }

    // =======================================================================
    // Code Embeddings (4)
    // =======================================================================

    /// Set embedding for a file node.
    pub async fn set_file_embedding(
        &self,
        file_path: &str,
        embedding: &[f32],
        model: &str,
    ) -> Result<()> {
        let emb_f64 = f32_slice_to_f64(embedding);
        let emb_json = serde_json::to_string(&emb_f64).unwrap_or_default();
        let rid = RecordId::new("file", file_path);
        self.db
            .query("UPDATE $rid SET embedding = $emb, embedding_model = $model RETURN NONE")
            .bind(("rid", rid))
            .bind(("emb", emb_json))
            .bind(("model", model.to_string()))
            .await
            .context("Failed to set file embedding")?;
        Ok(())
    }

    /// Set embedding for a function node.
    pub async fn set_function_embedding(
        &self,
        function_name: &str,
        file_path: &str,
        embedding: &[f32],
        model: &str,
    ) -> Result<()> {
        let emb_f64 = f32_slice_to_f64(embedding);
        let emb_json = serde_json::to_string(&emb_f64).unwrap_or_default();
        // Function ID is typically "name" or composite "file::name"
        let func_id = format!("{}::{}", file_path, function_name);
        self.db
            .query(
                "UPDATE function SET embedding = $emb, embedding_model = $model \
                 WHERE name = $fname AND file_path = $fpath RETURN NONE",
            )
            .bind(("emb", emb_json))
            .bind(("model", model.to_string()))
            .bind(("fname", function_name.to_string()))
            .bind(("fpath", file_path.to_string()))
            .await
            .context("Failed to set function embedding")?;
        // Fallback: also try by RecordId
        let rid = RecordId::new("function", func_id.as_str());
        let _ = self
            .db
            .query("UPDATE $rid SET embedding = $emb2, embedding_model = $model2 RETURN NONE")
            .bind(("rid", rid))
            .bind((
                "emb2",
                serde_json::to_string(&f32_slice_to_f64(embedding)).unwrap_or_default(),
            ))
            .bind(("model2", model.to_string()))
            .await;
        Ok(())
    }

    /// Search files by vector similarity (in-memory cosine fallback).
    pub async fn vector_search_files(
        &self,
        embedding: &[f32],
        limit: usize,
        project_id: Option<Uuid>,
    ) -> Result<Vec<(String, f64)>> {
        let sql = if project_id.is_some() {
            "SELECT * FROM file WHERE embedding != NONE AND project_id = $pid"
        } else {
            "SELECT * FROM file WHERE embedding != NONE"
        };
        let mut qb = self.db.query(sql);
        if let Some(pid) = project_id {
            qb = qb.bind(("pid", pid.to_string()));
        }
        let mut resp = qb.await.context("Failed to search files by vector")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;

        let query_f64: Vec<f64> = embedding.iter().map(|&x| x as f64).collect();
        let mut scored: Vec<(String, f64)> = Vec::new();

        for record in records {
            if let Some(emb_str) = record.get("embedding").and_then(|v| v.as_str()) {
                let emb: Vec<f64> = serde_json::from_str(emb_str).unwrap_or_default();
                if !emb.is_empty() {
                    let sim = cosine_similarity(&query_f64, &emb);
                    let path = record
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    scored.push((path, sim));
                }
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    /// Search functions by vector similarity (in-memory cosine fallback).
    pub async fn vector_search_functions(
        &self,
        embedding: &[f32],
        limit: usize,
        project_id: Option<Uuid>,
    ) -> Result<Vec<(String, String, f64)>> {
        // Functions don't have project_id directly, so filter via file.
        let sql = if project_id.is_some() {
            "SELECT * FROM function WHERE embedding != NONE \
             AND file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid)"
        } else {
            "SELECT * FROM function WHERE embedding != NONE"
        };
        let mut qb = self.db.query(sql);
        if let Some(pid) = project_id {
            qb = qb.bind(("pid", pid.to_string()));
        }
        let mut resp = qb.await.context("Failed to search functions by vector")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;

        let query_f64: Vec<f64> = embedding.iter().map(|&x| x as f64).collect();
        let mut scored: Vec<(String, String, f64)> = Vec::new();

        for record in records {
            if let Some(emb_str) = record.get("embedding").and_then(|v| v.as_str()) {
                let emb: Vec<f64> = serde_json::from_str(emb_str).unwrap_or_default();
                if !emb.is_empty() {
                    let sim = cosine_similarity(&query_f64, &emb);
                    let name = record
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let file_path = record
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    scored.push((name, file_path, sim));
                }
            }
        }

        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    // =======================================================================
    // Helpers (private)
    // =======================================================================

    /// Populate supersedes/superseded_by from edge table.
    async fn populate_note_succession(&self, note: &mut Note) -> Result<()> {
        let note_rid = RecordId::new("note", note.id.to_string().as_str());

        // supersedes: outgoing supersedes edges
        let mut resp_out = self
            .db
            .query("SELECT out FROM supersedes WHERE in = $note_rid")
            .bind(("note_rid", note_rid.clone()))
            .await
            .context("Failed to get supersedes edge")?;
        let out_records: Vec<SupersedesOutRecord> = resp_out.take(0)?;
        note.supersedes = out_records
            .into_iter()
            .next()
            .and_then(|r| rid_to_uuid(&r.out).ok());

        // superseded_by: note appears as `out` in another note's supersedes edge
        let mut resp_in = self
            .db
            .query(
                "SELECT * FROM note WHERE id IN \
                 (SELECT VALUE in.id FROM supersedes WHERE out = $note_rid) \
                 LIMIT 1",
            )
            .bind(("note_rid", note_rid))
            .await
            .context("Failed to get superseded_by")?;
        let in_records: Vec<NoteRecord> = resp_in.take(0)?;
        note.superseded_by = in_records
            .into_iter()
            .next()
            .and_then(|r| rid_to_uuid(&r.id).ok());

        Ok(())
    }

    // =======================================================================
    // Full-text search (BM25)
    // =======================================================================

    /// Full-text BM25 search across notes (content + tags).
    ///
    /// Uses SurrealDB's `@@` operator with the `cortex_analyzer` BM25 index.
    /// Falls back gracefully to a CONTAINS-based keyword search when the BM25
    /// index is unavailable (e.g. in-memory `kv-mem` engine used by tests).
    pub async fn search_notes_fts(
        &self,
        query: &str,
        limit: usize,
        project_id: Option<&str>,
    ) -> Result<Vec<(Note, f64)>> {
        self.ensure_note_schema_extensions().await?;

        // Build a BM25 query using the @@ full-text operator.
        // search::score() returns the BM25 relevance score for each row.
        let bm25_query = if project_id.is_some() {
            "SELECT meta::id(id) AS uid, project_id, note_type, status, \
             importance, content, tags, scope_type, scope_path, \
             staleness_score, energy, created_at, created_by, \
             confirmed_at, confirmed_by, last_activated, \
             changes_json, assertion_rule_json, assertion_result_json, \
             search::score() AS _score \
             FROM note \
             WHERE (content @@ $query OR tags @@ $query) \
               AND project_id = $project_id \
               AND status NOT IN ['obsolete', 'archived'] \
             ORDER BY _score DESC \
             LIMIT $limit"
        } else {
            "SELECT meta::id(id) AS uid, project_id, note_type, status, \
             importance, content, tags, scope_type, scope_path, \
             staleness_score, energy, created_at, created_by, \
             confirmed_at, confirmed_by, last_activated, \
             changes_json, assertion_rule_json, assertion_result_json, \
             search::score() AS _score \
             FROM note \
             WHERE (content @@ $query OR tags @@ $query) \
               AND status NOT IN ['obsolete', 'archived'] \
             ORDER BY _score DESC \
             LIMIT $limit"
        };

        let bm25_result: Option<Vec<serde_json::Value>> = {
            let mut qb = self
                .db
                .query(bm25_query)
                .bind(("query", query.to_string()))
                .bind(("limit", limit));
            if let Some(pid) = project_id {
                qb = qb.bind(("project_id", pid.to_string()));
            }
            match qb.await {
                Ok(mut resp) => resp.take(0).ok(),
                Err(e) => {
                    tracing::warn!(error = %e, "BM25 FTS unavailable, falling back to CONTAINS search");
                    None
                }
            }
        };

        if let Some(rows) = bm25_result {
            if !rows.is_empty() || query.len() > 1 {
                // BM25 path succeeded — convert rows to (Note, score) pairs.
                let mut results = Vec::new();
                for row in rows {
                    let score = row.get("_score").and_then(|v| v.as_f64()).unwrap_or(1.0);
                    if let Ok(note) = json_value_to_note(&row) {
                        results.push((note, score));
                    }
                }
                results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                results.truncate(limit);
                return Ok(results);
            }
        }

        // Fallback: keyword CONTAINS search (used in tests with kv-mem engine).
        let kw = query.to_lowercase();
        let fallback_sql = if project_id.is_some() {
            "SELECT meta::id(id) AS uid, project_id, note_type, status, \
             importance, content, tags, scope_type, scope_path, \
             staleness_score, energy, created_at, created_by, \
             confirmed_at, confirmed_by, last_activated, \
             changes_json, assertion_rule_json, assertion_result_json \
             FROM note \
             WHERE string::lowercase(content) CONTAINS $kw \
               AND project_id = $project_id \
               AND status NOT IN ['obsolete', 'archived'] \
             LIMIT $limit"
        } else {
            "SELECT meta::id(id) AS uid, project_id, note_type, status, \
             importance, content, tags, scope_type, scope_path, \
             staleness_score, energy, created_at, created_by, \
             confirmed_at, confirmed_by, last_activated, \
             changes_json, assertion_rule_json, assertion_result_json \
             FROM note \
             WHERE string::lowercase(content) CONTAINS $kw \
               AND status NOT IN ['obsolete', 'archived'] \
             LIMIT $limit"
        };
        let mut qb = self
            .db
            .query(fallback_sql)
            .bind(("kw", kw))
            .bind(("limit", limit as i64));
        if let Some(pid) = project_id {
            qb = qb.bind(("project_id", pid.to_string()));
        }
        let mut resp = qb
            .await
            .context("Failed to run FTS fallback search for notes")?;
        let rows: Vec<serde_json::Value> = resp.take(0)?;
        let mut results = Vec::new();
        for row in rows {
            if let Ok(note) = json_value_to_note(&row) {
                results.push((note, 1.0f64));
            }
        }
        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Utility: reconstruct Note from serde_json::Value
// ---------------------------------------------------------------------------

/// Build a Note from a serde_json::Value record (used when we can't use NoteRecord
/// because the `embedding` column is an array, not a string).
pub(crate) fn json_value_to_note(v: &serde_json::Value) -> Result<Note> {
    // Extract id — try `uid` first (from meta::id alias), then `id` as fallback
    let id_str = v
        .get("uid")
        .and_then(|id| id.as_str())
        .or_else(|| v.get("id").and_then(|id| id.as_str()))
        .unwrap_or("");
    // SurrealDB may return ids like "note:⟨uuid⟩" — extract just the UUID part
    let uuid_str = id_str
        .split(':')
        .next_back()
        .unwrap_or(id_str)
        .trim_start_matches('⟨')
        .trim_end_matches('⟩');
    let id = Uuid::parse_str(uuid_str).context("Failed to parse note id from JSON")?;

    let project_id = v
        .get("project_id")
        .and_then(|p| p.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    let note_type = parse_note_type(
        v.get("note_type")
            .and_then(|n| n.as_str())
            .unwrap_or("observation"),
    );
    let status = parse_note_status(v.get("status").and_then(|s| s.as_str()).unwrap_or("active"));
    let importance = parse_note_importance(
        v.get("importance")
            .and_then(|i| i.as_str())
            .unwrap_or("medium"),
    );
    let scope = scope_from_parts(
        v.get("scope_type").and_then(|s| s.as_str()),
        v.get("scope_path").and_then(|s| s.as_str()),
    );
    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let tags: Vec<String> = v
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let created_at = v
        .get("created_at")
        .and_then(|c| c.as_str())
        .and_then(parse_datetime)
        .unwrap_or_else(Utc::now);
    let created_by = v
        .get("created_by")
        .and_then(|c| c.as_str())
        .unwrap_or("unknown")
        .to_string();
    let last_confirmed_at = v
        .get("confirmed_at")
        .and_then(|c| c.as_str())
        .and_then(parse_datetime);
    let confirmed_by = v
        .get("confirmed_by")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());
    let staleness_score = v
        .get("staleness_score")
        .and_then(|s| s.as_f64())
        .unwrap_or(0.0);
    let energy = v.get("energy").and_then(|e| e.as_f64()).unwrap_or(1.0);
    let last_activated = v
        .get("last_activated")
        .and_then(|l| l.as_str())
        .and_then(parse_datetime);

    Ok(Note {
        id,
        project_id,
        note_type,
        status,
        importance,
        scope,
        content,
        tags,
        anchors: vec![],
        created_at,
        created_by,
        last_confirmed_at,
        last_confirmed_by: confirmed_by,
        staleness_score,
        energy,
        last_activated,
        supersedes: None,
        superseded_by: None,
        changes: vec![],
        valid_at: None,
        invalid_at: None,
        assertion_rule: None,
        last_assertion_result: None,
    })
}

// ---------------------------------------------------------------------------
// Utility: cosine similarity
// ---------------------------------------------------------------------------

fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let mag_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }
    dot / (mag_a * mag_b)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::notes::{NoteImportance, NoteScope, NoteStatus, NoteType};

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store.ensure_note_schema_extensions().await.unwrap();
        store
    }

    fn test_note() -> Note {
        Note::new(
            Some(Uuid::new_v4()),
            NoteType::Guideline,
            "Always use Result for error handling".to_string(),
            "test-agent".to_string(),
        )
    }

    fn test_note_full(project_id: Option<Uuid>) -> Note {
        Note::new_full(
            project_id,
            NoteType::Gotcha,
            NoteImportance::High,
            NoteScope::File("/src/main.rs".to_string()),
            "Watch out for unwrap() in production code".to_string(),
            vec!["rust".to_string(), "error-handling".to_string()],
            "test-agent".to_string(),
        )
    }

    #[tokio::test]
    async fn test_create_and_get_note() {
        let store = setup().await;
        let note = test_note();
        store.create_note(&note).await.unwrap();

        let retrieved = store.get_note(note.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, note.id);
        assert_eq!(retrieved.content, "Always use Result for error handling");
        assert_eq!(retrieved.note_type, NoteType::Guideline);
        assert_eq!(retrieved.status, NoteStatus::Active);
        assert_eq!(retrieved.importance, NoteImportance::Medium);
        assert_eq!(retrieved.created_by, "test-agent");
        assert!(retrieved.energy > 0.0);
    }

    #[tokio::test]
    async fn test_update_note() {
        let store = setup().await;
        let note = test_note();
        store.create_note(&note).await.unwrap();

        let updated = store
            .update_note(
                note.id,
                Some("Updated content".to_string()),
                Some(NoteImportance::Critical),
                Some(NoteStatus::NeedsReview),
                Some(vec!["updated".to_string()]),
                Some(0.5),
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(updated.content, "Updated content");
        assert_eq!(updated.importance, NoteImportance::Critical);
        assert_eq!(updated.status, NoteStatus::NeedsReview);
        assert_eq!(updated.tags, vec!["updated".to_string()]);
        assert!((updated.staleness_score - 0.5).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_delete_note() {
        let store = setup().await;
        let note = test_note();
        store.create_note(&note).await.unwrap();

        let deleted = store.delete_note(note.id).await.unwrap();
        assert!(deleted);

        let retrieved = store.get_note(note.id).await.unwrap();
        assert!(retrieved.is_none());

        // Deleting non-existent note returns false
        let deleted_again = store.delete_note(note.id).await.unwrap();
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn test_list_notes_with_filters() {
        let store = setup().await;
        let pid = Uuid::new_v4();

        // Create notes with different types and statuses
        let mut note1 = test_note_full(Some(pid));
        note1.status = NoteStatus::Active;
        store.create_note(&note1).await.unwrap();

        let mut note2 = Note::new(
            Some(pid),
            NoteType::Pattern,
            "Use builder pattern".to_string(),
            "agent".to_string(),
        );
        note2.importance = NoteImportance::Low;
        store.create_note(&note2).await.unwrap();

        let note3 = Note::new(
            None, // global note
            NoteType::Tip,
            "Global tip".to_string(),
            "agent".to_string(),
        );
        store.create_note(&note3).await.unwrap();

        // List all for project
        let (notes, total) = store
            .list_notes(Some(pid), None, &NoteFilters::default())
            .await
            .unwrap();
        assert_eq!(total, 2);
        assert_eq!(notes.len(), 2);

        // Filter by type
        let (notes, total) = store
            .list_notes(
                Some(pid),
                None,
                &NoteFilters {
                    note_type: Some(vec![NoteType::Gotcha]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(notes[0].note_type, NoteType::Gotcha);

        // Filter by importance
        let (notes, total) = store
            .list_notes(
                Some(pid),
                None,
                &NoteFilters {
                    importance: Some(vec![NoteImportance::High]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(notes[0].importance, NoteImportance::High);
    }

    #[tokio::test]
    async fn test_link_note_to_entity() {
        let store = setup().await;
        let note = test_note();
        store.create_note(&note).await.unwrap();

        // Create a file to link to
        let file = cortex_core::test_helpers::test_file("/src/main.rs");
        store.upsert_file(&file).await.unwrap();

        // Link note to file
        store
            .link_note_to_entity(
                note.id,
                &EntityType::File,
                "/src/main.rs",
                Some("sig123"),
                Some("body456"),
            )
            .await
            .unwrap();

        // Get notes for entity
        let notes = store
            .get_notes_for_entity(&EntityType::File, "/src/main.rs")
            .await
            .unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, note.id);

        // Get anchors
        let anchors = store.get_note_anchors(note.id).await.unwrap();
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].entity_type, EntityType::File);
        assert_eq!(anchors[0].entity_id, "/src/main.rs");
        assert_eq!(anchors[0].signature_hash, Some("sig123".to_string()));
        assert_eq!(anchors[0].body_hash, Some("body456".to_string()));

        // Unlink
        store
            .unlink_note_from_entity(note.id, &EntityType::File, "/src/main.rs")
            .await
            .unwrap();
        let notes_after = store
            .get_notes_for_entity(&EntityType::File, "/src/main.rs")
            .await
            .unwrap();
        assert!(notes_after.is_empty());
    }

    #[tokio::test]
    async fn test_supersede_note() {
        let store = setup().await;
        let old_note = test_note();
        store.create_note(&old_note).await.unwrap();

        let mut new_note = test_note();
        new_note.content = "Updated guideline".to_string();
        store.create_note(&new_note).await.unwrap();

        store
            .supersede_note(old_note.id, new_note.id)
            .await
            .unwrap();

        // Old note should be obsolete
        let old = store.get_note(old_note.id).await.unwrap().unwrap();
        assert_eq!(old.status, NoteStatus::Obsolete);

        // New note should reference old via supersedes
        let new = store.get_note(new_note.id).await.unwrap().unwrap();
        assert_eq!(new.supersedes, Some(old_note.id));

        // Old note should have superseded_by
        assert_eq!(old.superseded_by, Some(new_note.id));
    }

    #[tokio::test]
    async fn test_confirm_note() {
        let store = setup().await;
        let mut note = test_note();
        note.staleness_score = 0.7;
        note.status = NoteStatus::NeedsReview;
        store.create_note(&note).await.unwrap();

        let confirmed = store
            .confirm_note(note.id, "reviewer")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(confirmed.status, NoteStatus::Active);
        assert!((confirmed.staleness_score - 0.0).abs() < 0.001);
        assert_eq!(confirmed.last_confirmed_by, Some("reviewer".to_string()));
        assert!(confirmed.last_confirmed_at.is_some());
    }

    #[tokio::test]
    async fn test_note_embedding() {
        let store = setup().await;
        let note = test_note();
        store.create_note(&note).await.unwrap();

        // Initially no embedding
        let emb = store.get_note_embedding(note.id).await.unwrap();
        assert!(emb.is_none());

        // Set embedding
        let embedding: Vec<f32> = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        store
            .set_note_embedding(note.id, &embedding, "test-model")
            .await
            .unwrap();

        // Get embedding
        let retrieved = store.get_note_embedding(note.id).await.unwrap().unwrap();
        assert_eq!(retrieved.len(), 5);
        assert!((retrieved[0] - 0.1).abs() < 0.001);
        assert!((retrieved[4] - 0.5).abs() < 0.001);

        // Should not appear in "without embedding" list
        let (without, _total) = store.list_notes_without_embedding(100, 0).await.unwrap();
        assert!(without.iter().all(|n| n.id != note.id));

        // Vector search
        let query_emb: Vec<f32> = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        let results = store
            .vector_search_notes(&query_emb, 10, None, None, None)
            .await
            .unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0.id, note.id);
        assert!(results[0].1 > 0.99); // should be ~1.0 for identical vectors
    }

    #[tokio::test]
    async fn test_create_and_get_synapses() {
        let store = setup().await;

        let note1 = test_note();
        let note2 = test_note();
        let note3 = test_note();
        store.create_note(&note1).await.unwrap();
        store.create_note(&note2).await.unwrap();
        store.create_note(&note3).await.unwrap();

        // Create synapses
        let created = store
            .create_synapses(note1.id, &[(note2.id, 0.8), (note3.id, 0.5)])
            .await
            .unwrap();
        assert_eq!(created, 2);

        // Get synapses
        let synapses = store.get_synapses(note1.id).await.unwrap();
        assert_eq!(synapses.len(), 2);

        let weights: Vec<f64> = synapses.iter().map(|(_, w)| *w).collect();
        assert!(weights.contains(&0.8));
        assert!(weights.contains(&0.5));

        let targets: Vec<Uuid> = synapses.iter().map(|(id, _)| *id).collect();
        assert!(targets.contains(&note2.id));
        assert!(targets.contains(&note3.id));

        // Delete synapses
        let deleted = store.delete_synapses(note1.id).await.unwrap();
        assert_eq!(deleted, 2);

        let after_delete = store.get_synapses(note1.id).await.unwrap();
        assert!(after_delete.is_empty());
    }

    #[tokio::test]
    async fn test_energy_operations() {
        let store = setup().await;
        let note = test_note();
        store.create_note(&note).await.unwrap();

        // Initial energy should be 1.0
        let retrieved = store.get_note(note.id).await.unwrap().unwrap();
        assert!((retrieved.energy - 1.0).abs() < 0.01);

        // Boost energy (should clamp at 1.0 since already at 1.0)
        store.boost_energy(note.id, 0.5).await.unwrap();
        let boosted = store.get_note(note.id).await.unwrap().unwrap();
        assert!((boosted.energy - 1.0).abs() < 0.01); // clamped at 1.0

        // Update energy scores (should not change much since note is fresh)
        let updated = store.update_energy_scores(90.0).await.unwrap();
        assert!(updated > 0);

        // Init note energy
        let init_count = store.init_note_energy().await.unwrap();
        // All notes already have energy, so count should be 0
        assert_eq!(init_count, 0);
    }

    #[tokio::test]
    async fn test_staleness_scores() {
        let store = setup().await;

        let note = test_note();
        store.create_note(&note).await.unwrap();

        // Update staleness scores (note is fresh, score should stay low)
        let updated = store.update_staleness_scores().await.unwrap();
        assert!(updated > 0);

        let after = store.get_note(note.id).await.unwrap().unwrap();
        // Note was just created, staleness should be very low
        assert!(after.staleness_score < 0.1);
    }

    #[tokio::test]
    async fn test_notes_needing_review() {
        let store = setup().await;
        let pid = Uuid::new_v4();

        // Create active note
        let active_note = Note::new(
            Some(pid),
            NoteType::Guideline,
            "Active note".to_string(),
            "agent".to_string(),
        );
        store.create_note(&active_note).await.unwrap();

        // Create needs_review note
        let mut review_note = Note::new(
            Some(pid),
            NoteType::Gotcha,
            "Needs review".to_string(),
            "agent".to_string(),
        );
        review_note.status = NoteStatus::NeedsReview;
        store.create_note(&review_note).await.unwrap();

        // Create stale note
        let mut stale_note = Note::new(
            Some(pid),
            NoteType::Tip,
            "Stale tip".to_string(),
            "agent".to_string(),
        );
        stale_note.status = NoteStatus::Stale;
        store.create_note(&stale_note).await.unwrap();

        let needing = store.get_notes_needing_review(Some(pid)).await.unwrap();
        assert_eq!(needing.len(), 2);
        let ids: Vec<Uuid> = needing.iter().map(|n| n.id).collect();
        assert!(ids.contains(&review_note.id));
        assert!(ids.contains(&stale_note.id));

        // Without project filter
        let all_needing = store.get_notes_needing_review(None).await.unwrap();
        assert_eq!(all_needing.len(), 2);
    }

    #[tokio::test]
    async fn test_search_notes_fts() {
        let store = setup().await;

        // Create a note with a distinctive content phrase
        let relevant_note = Note::new(
            None,
            NoteType::Pattern,
            "authentication middleware pattern for token validation".to_string(),
            "agent".to_string(),
        );
        store.create_note(&relevant_note).await.unwrap();

        // Create a completely unrelated note
        let unrelated_note = Note::new(
            None,
            NoteType::Tip,
            "always use cargo fmt before committing code".to_string(),
            "agent".to_string(),
        );
        store.create_note(&unrelated_note).await.unwrap();

        // Search for the distinctive phrase — kv-mem engine falls back to CONTAINS
        let results = store
            .search_notes_fts("authentication middleware", 10, None)
            .await
            .unwrap();

        assert!(
            !results.is_empty(),
            "search should return at least one result"
        );

        let result_ids: Vec<Uuid> = results.iter().map(|(n, _)| n.id).collect();
        assert!(
            result_ids.contains(&relevant_note.id),
            "relevant note should appear in FTS results"
        );
        assert!(
            !result_ids.contains(&unrelated_note.id),
            "unrelated note should NOT appear in FTS results"
        );
    }
}
