//! Decision CRUD operations for IndentiaGraphStore.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{AffectsRelation, DecisionNode, DecisionStatus, DecisionTimelineEntry};
use std::collections::HashMap;
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

// ---------------------------------------------------------------------------
// Record types (module-level for SurrealValue derive)
// ---------------------------------------------------------------------------

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
    task_id: Option<String>,
    embedding: Option<String>,
    embedding_model: Option<String>,
}

#[derive(Debug, SurrealValue)]
struct AffectsRecord {
    entity_type: Option<String>,
    entity_id: Option<String>,
    entity_name: Option<String>,
    impact_description: Option<String>,
}

#[derive(Debug, SurrealValue)]
struct SupersedesOutRecord {
    out: RecordId,
}

// Note: We use serde_json::Value for incoming supersedes queries because
// `in` is a Rust keyword and #[derive(SurrealValue)] does not handle r#in.

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn parse_decision_status(s: &str) -> DecisionStatus {
    match s {
        "proposed" => DecisionStatus::Proposed,
        "accepted" => DecisionStatus::Accepted,
        "deprecated" => DecisionStatus::Deprecated,
        "superseded" => DecisionStatus::Superseded,
        _ => DecisionStatus::Proposed,
    }
}

fn status_to_str(s: &DecisionStatus) -> &'static str {
    match s {
        DecisionStatus::Proposed => "proposed",
        DecisionStatus::Accepted => "accepted",
        DecisionStatus::Deprecated => "deprecated",
        DecisionStatus::Superseded => "superseded",
    }
}

impl DecisionRecord {
    fn into_node(self) -> Result<DecisionNode> {
        let alternatives: Vec<String> = self
            .alternatives
            .as_ref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        let embedding: Option<Vec<f64>> = self
            .embedding
            .as_ref()
            .and_then(|s| serde_json::from_str(s).ok());

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
            embedding,
            embedding_model: self.embedding_model,
        })
    }
}

fn vec_to_json_str(v: &[String]) -> Option<String> {
    if v.is_empty() {
        None
    } else {
        Some(serde_json::to_string(v).unwrap_or_default())
    }
}

fn embedding_to_json_str(v: &[f64]) -> Option<String> {
    if v.is_empty() {
        None
    } else {
        Some(serde_json::to_string(v).unwrap_or_default())
    }
}

fn f32_slice_to_f64(v: &[f32]) -> Vec<f64> {
    v.iter().map(|&x| x as f64).collect()
}

fn f64_vec_to_f32(v: &[f64]) -> Vec<f32> {
    v.iter().map(|&x| x as f32).collect()
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl IndentiaGraphStore {
    // -----------------------------------------------------------------------
    // Core CRUD
    // -----------------------------------------------------------------------

    /// Create a decision and link it to a task via informed_by edge.
    pub async fn create_decision(&self, task_id: Uuid, decision: &DecisionNode) -> Result<()> {
        let rid = RecordId::new("decision", decision.id.to_string().as_str());
        let emb_str = decision
            .embedding
            .as_ref()
            .and_then(|e| embedding_to_json_str(e));

        self.db
            .query(
                "CREATE $rid SET \
                 description = $desc, rationale = $rat, \
                 alternatives = $alts, chosen_option = $co, \
                 decided_by = $db, decided_at = $da, \
                 status = $status, task_id = $tid, \
                 embedding = $emb, embedding_model = $emb_model \
                 RETURN NONE",
            )
            .bind(("rid", rid.clone()))
            .bind(("desc", decision.description.clone()))
            .bind(("rat", decision.rationale.clone()))
            .bind(("alts", vec_to_json_str(&decision.alternatives)))
            .bind(("co", decision.chosen_option.clone()))
            .bind(("db", decision.decided_by.clone()))
            .bind(("da", decision.decided_at.to_rfc3339()))
            .bind(("status", status_to_str(&decision.status).to_string()))
            .bind(("tid", Some(task_id.to_string())))
            .bind(("emb", emb_str))
            .bind(("emb_model", decision.embedding_model.clone()))
            .await
            .context("Failed to create decision")?;

        // Create informed_by edge: task -> informed_by -> decision
        let task_rid = RecordId::new("task", task_id.to_string().as_str());
        let dec_rid = RecordId::new("decision", decision.id.to_string().as_str());
        self.db
            .query("RELATE $from->informed_by->$to RETURN NONE")
            .bind(("from", task_rid))
            .bind(("to", dec_rid))
            .await
            .context("Failed to create informed_by edge")?;

        Ok(())
    }

    /// Get a decision by ID.
    pub async fn get_decision(&self, decision_id: Uuid) -> Result<Option<DecisionNode>> {
        let rid = RecordId::new("decision", decision_id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get decision")?;
        let records: Vec<DecisionRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    /// Update a decision (partial update).
    pub async fn update_decision(
        &self,
        decision_id: Uuid,
        description: Option<String>,
        rationale: Option<String>,
        chosen_option: Option<String>,
        status: Option<DecisionStatus>,
    ) -> Result<()> {
        let mut sets = Vec::new();
        if description.is_some() {
            sets.push("description = $desc");
        }
        if rationale.is_some() {
            sets.push("rationale = $rat");
        }
        if chosen_option.is_some() {
            sets.push("chosen_option = $co");
        }
        if status.is_some() {
            sets.push("status = $status");
        }
        if sets.is_empty() {
            return Ok(());
        }

        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let rid = RecordId::new("decision", decision_id.to_string().as_str());
        let mut q = self.db.query(&query);
        q = q.bind(("rid", rid));
        if let Some(ref d) = description {
            q = q.bind(("desc", d.clone()));
        }
        if let Some(ref r) = rationale {
            q = q.bind(("rat", r.clone()));
        }
        if let Some(ref c) = chosen_option {
            q = q.bind(("co", c.clone()));
        }
        if let Some(ref s) = status {
            q = q.bind(("status", status_to_str(s).to_string()));
        }

        q.await.context("Failed to update decision")?;
        Ok(())
    }

    /// Delete a decision and all associated edges (affects, supersedes, informed_by).
    pub async fn delete_decision(&self, decision_id: Uuid) -> Result<()> {
        let did = decision_id.to_string();
        let rid = RecordId::new("decision", did.as_str());
        self.db
            .query(
                "DELETE FROM informed_by WHERE out = type::record('decision', $did);\
                 DELETE FROM affects WHERE in = type::record('decision', $did);\
                 DELETE FROM supersedes WHERE in = type::record('decision', $did) OR out = type::record('decision', $did);\
                 DELETE $rid",
            )
            .bind(("did", did))
            .bind(("rid", rid))
            .await
            .context("Failed to delete decision")?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Get decisions affecting an entity (via affects edge).
    pub async fn get_decisions_for_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
        limit: u32,
    ) -> Result<Vec<DecisionNode>> {
        // affects edges store entity_type and entity_id on the edge itself
        let mut resp = self
            .db
            .query(
                "SELECT * FROM decision WHERE id IN \
                 (SELECT VALUE in.id FROM affects \
                  WHERE entity_type = $etype AND entity_id = $eid) \
                 LIMIT $limit",
            )
            .bind(("etype", entity_type.to_string()))
            .bind(("eid", entity_id.to_string()))
            .bind(("limit", limit as i64))
            .await
            .context("Failed to get decisions for entity")?;
        let records: Vec<DecisionRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    /// Get decisions affecting an entity with optional status filter.
    pub async fn get_decisions_affecting(
        &self,
        entity_type: &str,
        entity_id: &str,
        status_filter: Option<&str>,
    ) -> Result<Vec<DecisionNode>> {
        let sql = if status_filter.is_some() {
            "SELECT * FROM decision WHERE \
             id IN (SELECT VALUE in.id FROM affects \
                    WHERE entity_type = $etype AND entity_id = $eid) \
             AND status = $status"
        } else {
            "SELECT * FROM decision WHERE \
             id IN (SELECT VALUE in.id FROM affects \
                    WHERE entity_type = $etype AND entity_id = $eid)"
        };
        let mut qb = self
            .db
            .query(sql)
            .bind(("etype", entity_type.to_string()))
            .bind(("eid", entity_id.to_string()));
        if let Some(status) = status_filter {
            qb = qb.bind(("status", status.to_string()));
        }
        let mut resp = qb.await.context("Failed to get decisions affecting")?;
        let records: Vec<DecisionRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    /// Get decision timeline, optionally filtered by task and date range.
    pub async fn get_decision_timeline(
        &self,
        task_id: Option<Uuid>,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<Vec<DecisionTimelineEntry>> {
        let mut conditions = Vec::new();
        if task_id.is_some() {
            conditions.push("task_id = $task_id".to_string());
        }
        if from.is_some() {
            conditions.push("decided_at >= $from_date".to_string());
        }
        if to.is_some() {
            conditions.push("decided_at <= $to_date".to_string());
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let query = format!(
            "SELECT * FROM decision {} ORDER BY decided_at ASC",
            where_clause
        );
        let mut qb = self.db.query(&query);
        if let Some(tid) = task_id {
            qb = qb.bind(("task_id", tid.to_string()));
        }
        if let Some(from_date) = from {
            qb = qb.bind(("from_date", from_date.to_string()));
        }
        if let Some(to_date) = to {
            qb = qb.bind(("to_date", to_date.to_string()));
        }
        let mut resp = qb.await.context("Failed to get decision timeline")?;
        let records: Vec<DecisionRecord> = resp.take(0)?;

        let mut entries = Vec::new();
        for record in records {
            let decision = record.into_node()?;
            let did = decision.id.to_string();

            // Get decisions this one supersedes (outgoing supersedes edges)
            let mut sup_resp = self
                .db
                .query("SELECT out FROM supersedes WHERE in = type::record('decision', $did)")
                .bind(("did", did.clone()))
                .await
                .context("Failed to get supersedes chain")?;
            let sup_records: Vec<SupersedesOutRecord> = sup_resp.take(0)?;
            let supersedes_chain: Vec<Uuid> = sup_records
                .into_iter()
                .filter_map(|r| rid_to_uuid(&r.out).ok())
                .collect();

            // Get decision that supersedes this one (incoming supersedes edges).
            // Use a subquery to look up the full decision record to avoid
            // deserializing the `in` keyword field from relation edges.
            let mut by_resp = self
                .db
                .query(
                    "SELECT * FROM decision WHERE id IN \
                     (SELECT VALUE in.id FROM supersedes WHERE out = type::record('decision', $did)) \
                     LIMIT 1",
                )
                .bind(("did", did))
                .await
                .context("Failed to get superseded_by")?;
            let by_records: Vec<DecisionRecord> = by_resp.take(0)?;
            let superseded_by = by_records
                .into_iter()
                .next()
                .and_then(|r| rid_to_uuid(&r.id).ok());

            entries.push(DecisionTimelineEntry {
                decision,
                supersedes_chain,
                superseded_by,
            });
        }

        Ok(entries)
    }

    // -----------------------------------------------------------------------
    // Embeddings
    // -----------------------------------------------------------------------

    /// Set the vector embedding for a decision.
    pub async fn set_decision_embedding(
        &self,
        decision_id: Uuid,
        embedding: &[f32],
        model: &str,
    ) -> Result<()> {
        let emb_f64 = f32_slice_to_f64(embedding);
        let emb_json = serde_json::to_string(&emb_f64).unwrap_or_default();
        let rid = RecordId::new("decision", decision_id.to_string().as_str());
        self.db
            .query("UPDATE $rid SET embedding = $emb, embedding_model = $model RETURN NONE")
            .bind(("rid", rid))
            .bind(("emb", emb_json))
            .bind(("model", model.to_string()))
            .await
            .context("Failed to set decision embedding")?;
        Ok(())
    }

    /// Get the vector embedding for a decision.
    pub async fn get_decision_embedding(&self, decision_id: Uuid) -> Result<Option<Vec<f32>>> {
        let rid = RecordId::new("decision", decision_id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT embedding FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get decision embedding")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        if let Some(record) = records.first() {
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

    /// Get decisions that do not have embeddings yet.
    pub async fn get_decisions_without_embedding(&self) -> Result<Vec<(Uuid, String, String)>> {
        let mut resp = self
            .db
            .query("SELECT * FROM decision WHERE embedding = NONE OR embedding = ''")
            .await
            .context("Failed to get decisions without embedding")?;
        let records: Vec<DecisionRecord> = resp.take(0)?;
        let mut results = Vec::new();
        for r in records {
            let id = rid_to_uuid(&r.id)?;
            results.push((id, r.description, r.rationale));
        }
        Ok(results)
    }

    /// Search decisions by vector similarity.
    ///
    /// Since SurrealDB in-memory mode does not support native vector search,
    /// we load all decisions with embeddings and compute cosine similarity in Rust.
    pub async fn search_decisions_by_vector(
        &self,
        query_embedding: &[f32],
        limit: usize,
        _project_id: Option<&str>,
    ) -> Result<Vec<(DecisionNode, f64)>> {
        let mut resp = self
            .db
            .query("SELECT * FROM decision WHERE embedding != NONE AND embedding != ''")
            .await
            .context("Failed to search decisions by vector")?;
        let records: Vec<DecisionRecord> = resp.take(0)?;

        let query_f64: Vec<f64> = query_embedding.iter().map(|&x| x as f64).collect();
        let mut scored: Vec<(DecisionNode, f64)> = Vec::new();

        for record in records {
            let emb_opt: Option<Vec<f64>> = record
                .embedding
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok());
            if let Some(ref emb) = emb_opt {
                let sim = cosine_similarity(&query_f64, emb);
                let node = record.into_node()?;
                scored.push((node, sim));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    // -----------------------------------------------------------------------
    // AFFECTS edges
    // -----------------------------------------------------------------------

    /// Add an AFFECTS edge from a decision to a generic entity.
    pub async fn add_decision_affects(
        &self,
        decision_id: Uuid,
        entity_type: &str,
        entity_id: &str,
        impact_description: Option<&str>,
    ) -> Result<()> {
        // The `out` side of the affects edge uses a generic record.
        // We key the affects table by a deterministic composite key so we can
        // look it up later without needing a real node on the `out` side.
        // Use a placeholder record (decision table itself) as the out target,
        // storing entity_type + entity_id as edge properties.
        let dec_rid = RecordId::new("decision", decision_id.to_string().as_str());
        // For the out side, we use a synthetic record: entity_type:entity_id
        // This allows the relation to exist even without a matching node table.
        let out_rid = RecordId::new(entity_type, entity_id);
        self.db
            .query(
                "RELATE $from->affects->$to SET \
                 entity_type = $et, entity_id = $eid, \
                 entity_name = $en, impact_description = $imp \
                 RETURN NONE",
            )
            .bind(("from", dec_rid))
            .bind(("to", out_rid))
            .bind(("et", entity_type.to_string()))
            .bind(("eid", entity_id.to_string()))
            .bind(("en", Option::<String>::None))
            .bind(("imp", impact_description.map(|s| s.to_string())))
            .await
            .context("Failed to add decision affects")?;
        Ok(())
    }

    /// Remove an AFFECTS edge from a decision to a specific entity.
    pub async fn remove_decision_affects(
        &self,
        decision_id: Uuid,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<()> {
        let did = decision_id.to_string();
        self.db
            .query(
                "DELETE FROM affects WHERE \
                 in = type::record('decision', $did) AND \
                 entity_type = $et AND entity_id = $eid",
            )
            .bind(("did", did))
            .bind(("et", entity_type.to_string()))
            .bind(("eid", entity_id.to_string()))
            .await
            .context("Failed to remove decision affects")?;
        Ok(())
    }

    /// List all AFFECTS relations for a decision.
    pub async fn list_decision_affects(&self, decision_id: Uuid) -> Result<Vec<AffectsRelation>> {
        let did = decision_id.to_string();
        let mut resp = self
            .db
            .query(
                "SELECT entity_type, entity_id, entity_name, impact_description \
                 FROM affects WHERE in = type::record('decision', $did)",
            )
            .bind(("did", did))
            .await
            .context("Failed to list decision affects")?;
        let records: Vec<AffectsRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .map(|r| AffectsRelation {
                entity_type: r.entity_type.unwrap_or_default(),
                entity_id: r.entity_id.unwrap_or_default(),
                entity_name: r.entity_name,
                impact_description: r.impact_description,
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Supersede an old decision with a new one.
    /// Creates a supersedes edge and marks the old decision as Superseded.
    pub async fn supersede_decision(
        &self,
        new_decision_id: Uuid,
        old_decision_id: Uuid,
    ) -> Result<()> {
        let new_rid = RecordId::new("decision", new_decision_id.to_string().as_str());
        let old_rid = RecordId::new("decision", old_decision_id.to_string().as_str());
        self.db
            .query(
                "RELATE $from->supersedes->$to RETURN NONE;\
                 UPDATE $old SET status = 'superseded' RETURN NONE",
            )
            .bind(("from", new_rid))
            .bind(("to", old_rid.clone()))
            .bind(("old", old_rid))
            .await
            .context("Failed to supersede decision")?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Batch
    // -----------------------------------------------------------------------

    /// Get all decisions along with their task_id.
    pub async fn get_all_decisions_with_task_id(&self) -> Result<Vec<(DecisionNode, Uuid)>> {
        let mut resp = self
            .db
            .query("SELECT * FROM decision WHERE task_id != NONE ORDER BY decided_at ASC")
            .await
            .context("Failed to get all decisions with task_id")?;
        let records: Vec<DecisionRecord> = resp.take(0)?;
        let mut results = Vec::new();
        for r in records {
            let task_id_str = r.task_id.clone().unwrap_or_default();
            let node = r.into_node()?;
            if let Ok(tid) = Uuid::parse_str(&task_id_str) {
                results.push((node, tid));
            }
        }
        Ok(results)
    }

    /// List decisions that have embeddings but no synapse edges.
    pub async fn list_decisions_needing_synapses(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<DecisionNode>, usize)> {
        // Count total decisions with embedding but no synapses
        let mut count_resp = self
            .db
            .query(
                "SELECT count() AS total FROM decision \
                 WHERE embedding != NONE AND embedding != '' \
                 AND id NOT IN (SELECT VALUE in.id FROM synapse WHERE in.id IS NOT NONE) \
                 GROUP ALL",
            )
            .await
            .context("Failed to count decisions needing synapses")?;
        let count_result: Vec<serde_json::Value> = count_resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let query = format!(
            "SELECT * FROM decision \
             WHERE embedding != NONE AND embedding != '' \
             AND id NOT IN (SELECT VALUE in.id FROM synapse WHERE in.id IS NOT NONE) \
             ORDER BY decided_at ASC LIMIT {} START {}",
            limit, offset
        );
        let mut resp = self
            .db
            .query(&query)
            .await
            .context("Failed to list decisions needing synapses")?;
        let records: Vec<DecisionRecord> = resp.take(0)?;
        let decisions: Result<Vec<DecisionNode>> =
            records.into_iter().map(|r| r.into_node()).collect();
        Ok((decisions?, total))
    }

    // =======================================================================
    // Full-text search (BM25)
    // =======================================================================

    /// Full-text BM25 search across decisions (description + rationale).
    ///
    /// Uses SurrealDB's `@@` operator with the `cortex_analyzer` BM25 index.
    /// Falls back to a CONTAINS-based keyword search when BM25 is unavailable
    /// (e.g. in-memory `kv-mem` engine used by tests).
    pub async fn search_decisions_fts(
        &self,
        query: &str,
        limit: usize,
        project_id: Option<&str>,
    ) -> anyhow::Result<Vec<(DecisionNode, f64)>> {
        let bm25_result: Option<Vec<serde_json::Value>> = {
            let surql = "SELECT meta::id(id) AS uid, description, rationale, \
                alternatives, chosen_option, decided_by, decided_at, status, \
                task_id, embedding_model, \
                search::score() AS _score \
                FROM decision \
                WHERE description @@ $query OR rationale @@ $query \
                ORDER BY _score DESC \
                LIMIT $limit";
            let qb = self
                .db
                .query(surql)
                .bind(("query", query.to_string()))
                .bind(("limit", limit));
            match qb.await {
                Ok(mut resp) => resp.take(0).ok(),
                Err(e) => {
                    tracing::warn!(error = %e, "BM25 FTS unavailable, falling back to CONTAINS search");
                    None
                }
            }
        };

        if let Some(rows) = bm25_result {
            if !rows.is_empty() {
                let mut results = Vec::new();
                for row in rows {
                    let score = row.get("_score").and_then(|v| v.as_f64()).unwrap_or(1.0);
                    if let Ok(node) = json_value_to_decision_node(&row) {
                        results.push((node, score));
                    }
                }
                results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                let mut filtered = self
                    .filter_decisions_by_project(results, project_id)
                    .await?;
                filtered.truncate(limit);
                return Ok(filtered);
            }
            tracing::debug!("BM25 returned no decision hits, falling back to CONTAINS search");
        }

        // Fallback: keyword CONTAINS search.
        let kw = query.to_lowercase();
        let fallback_sql = "SELECT * FROM decision \
             WHERE string::lowercase(description) CONTAINS $kw \
                OR string::lowercase(rationale) CONTAINS $kw \
             LIMIT $limit";
        let mut resp = self
            .db
            .query(fallback_sql)
            .bind(("kw", kw))
            .bind(("limit", limit as i64))
            .await
            .context("Failed to run FTS fallback search for decisions")?;
        let records: Vec<DecisionRecord> = resp.take(0)?;
        let mut results = Vec::new();
        for r in records {
            if let Ok(node) = r.into_node() {
                results.push((node, 1.0f64));
            }
        }
        if results.is_empty() {
            // kv-mem can return no rows for CONTAINS in some test setups.
            // Fall back to in-memory keyword filtering for deterministic behavior.
            let q = query.to_lowercase();
            let all = self.get_all_decisions_with_task_id().await?;
            results = all
                .into_iter()
                .map(|(decision, _)| decision)
                .filter(|d| {
                    d.description.to_lowercase().contains(&q)
                        || d.rationale.to_lowercase().contains(&q)
                })
                .map(|d| (d, 1.0f64))
                .collect();
        }
        let mut filtered = self
            .filter_decisions_by_project(results, project_id)
            .await?;
        filtered.truncate(limit);
        Ok(filtered)
    }

    async fn filter_decisions_by_project(
        &self,
        results: Vec<(DecisionNode, f64)>,
        project_id: Option<&str>,
    ) -> anyhow::Result<Vec<(DecisionNode, f64)>> {
        let Some(project_id) = project_id else {
            return Ok(results);
        };
        let project_uuid =
            Uuid::parse_str(project_id).context("Invalid project_id in search_decisions_fts")?;
        let project_id_str = project_uuid.to_string();
        let decision_task_map: HashMap<Uuid, Uuid> = self
            .get_all_decisions_with_task_id()
            .await?
            .into_iter()
            .map(|(decision, task_id)| (decision.id, task_id))
            .collect();
        let mut task_project_match_cache: HashMap<Uuid, bool> = HashMap::new();
        let mut filtered = Vec::new();
        for (decision, score) in results {
            if let Some(task_id) = decision_task_map.get(&decision.id) {
                let belongs = if let Some(cached) = task_project_match_cache.get(task_id) {
                    *cached
                } else {
                    let task_rid = RecordId::new("task", task_id.to_string().as_str());
                    let mut resp = self
                        .db
                        .query("SELECT VALUE plan_id FROM $task_rid LIMIT 1")
                        .bind(("task_rid", task_rid))
                        .await
                        .context("Failed to resolve task plan_id for decision project filter")?;
                    let task_plan_ids: Vec<String> = resp.take(0).unwrap_or_default();
                    let belongs = if let Some(plan_id) = task_plan_ids.first() {
                        let plan_rid = RecordId::new("plan", plan_id.as_str());
                        let mut resp = self
                            .db
                            .query("SELECT VALUE project_id FROM $plan_rid LIMIT 1")
                            .bind(("plan_rid", plan_rid))
                            .await
                            .context(
                                "Failed to resolve plan project_id for decision project filter",
                            )?;
                        let plan_project_ids: Vec<String> = resp.take(0).unwrap_or_default();
                        plan_project_ids.first() == Some(&project_id_str)
                    } else {
                        false
                    };
                    task_project_match_cache.insert(*task_id, belongs);
                    belongs
                };
                if belongs {
                    filtered.push((decision, score));
                }
            }
        }
        Ok(filtered)
    }
}

// ---------------------------------------------------------------------------
// Utility: reconstruct DecisionNode from serde_json::Value
// ---------------------------------------------------------------------------

/// Build a DecisionNode from a serde_json::Value row (used for BM25 search
/// results where we can't use the SurrealValue-derived DecisionRecord because
/// the result set includes the extra `_score` column and uses `meta::id` alias).
fn json_value_to_decision_node(v: &serde_json::Value) -> anyhow::Result<DecisionNode> {
    use anyhow::Context as _;

    let id_str = v
        .get("uid")
        .and_then(|id| id.as_str())
        .or_else(|| v.get("id").and_then(|id| id.as_str()))
        .unwrap_or("");
    // SurrealDB may return ids like "decision:⟨uuid⟩" — extract just the UUID part.
    let uuid_str = id_str
        .split(':')
        .next_back()
        .unwrap_or(id_str)
        .trim_start_matches('⟨')
        .trim_end_matches('⟩');
    let id = Uuid::parse_str(uuid_str).context("Failed to parse decision id")?;

    let description = v
        .get("description")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let rationale = v
        .get("rationale")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let alternatives: Vec<String> = v
        .get("alternatives")
        .and_then(|s| s.as_str())
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let chosen_option = v
        .get("chosen_option")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let decided_by = v
        .get("decided_by")
        .and_then(|s| s.as_str())
        .unwrap_or("unknown")
        .to_string();
    let decided_at: DateTime<Utc> = v
        .get("decided_at")
        .and_then(|s| s.as_str())
        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        .unwrap_or_else(Utc::now);
    let status = parse_decision_status(
        v.get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("proposed"),
    );
    let embedding_model = v
        .get("embedding_model")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());

    Ok(DecisionNode {
        id,
        description,
        rationale,
        alternatives,
        chosen_option,
        decided_by,
        decided_at,
        status,
        embedding: None, // Not fetched in BM25 query to keep payload small
        embedding_model,
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
    use cortex_core::test_helpers::{test_decision, test_plan, test_project_named, test_task};

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_create_and_get_decision() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Decision task");
        store.create_task(plan.id, &task).await.unwrap();

        let decision = test_decision();
        store.create_decision(task.id, &decision).await.unwrap();

        let retrieved = store.get_decision(decision.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, decision.id);
        assert_eq!(retrieved.description, "Use SurrealDB");
        assert_eq!(retrieved.rationale, "Better Rust integration");
        assert_eq!(retrieved.decided_by, "test-architect");
        assert_eq!(retrieved.status, DecisionStatus::Proposed);
        assert_eq!(
            retrieved.alternatives,
            vec!["PostgreSQL".to_string(), "MongoDB".to_string()]
        );
    }

    #[tokio::test]
    async fn test_get_nonexistent_decision() {
        let store = setup().await;
        let result = store.get_decision(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_update_decision() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Update decision task");
        store.create_task(plan.id, &task).await.unwrap();

        let decision = test_decision();
        store.create_decision(task.id, &decision).await.unwrap();

        store
            .update_decision(
                decision.id,
                Some("Use PostgreSQL".to_string()),
                None,
                Some("PostgreSQL with AGE".to_string()),
                Some(DecisionStatus::Accepted),
            )
            .await
            .unwrap();

        let updated = store.get_decision(decision.id).await.unwrap().unwrap();
        assert_eq!(updated.description, "Use PostgreSQL");
        assert_eq!(updated.rationale, "Better Rust integration"); // unchanged
        assert_eq!(
            updated.chosen_option,
            Some("PostgreSQL with AGE".to_string())
        );
        assert_eq!(updated.status, DecisionStatus::Accepted);
    }

    #[tokio::test]
    async fn test_delete_decision() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Delete decision task");
        store.create_task(plan.id, &task).await.unwrap();

        let decision = test_decision();
        store.create_decision(task.id, &decision).await.unwrap();

        store.delete_decision(decision.id).await.unwrap();
        assert!(store.get_decision(decision.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_decision_affects() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Affects task");
        store.create_task(plan.id, &task).await.unwrap();

        let decision = test_decision();
        store.create_decision(task.id, &decision).await.unwrap();

        // Add affects
        store
            .add_decision_affects(
                decision.id,
                "file",
                "/src/main.rs",
                Some("Database driver change"),
            )
            .await
            .unwrap();
        store
            .add_decision_affects(decision.id, "function", "connect_db", None)
            .await
            .unwrap();

        // List affects
        let affects = store.list_decision_affects(decision.id).await.unwrap();
        assert_eq!(affects.len(), 2);

        let file_affect = affects.iter().find(|a| a.entity_type == "file").unwrap();
        assert_eq!(file_affect.entity_id, "/src/main.rs");
        assert_eq!(
            file_affect.impact_description,
            Some("Database driver change".to_string())
        );

        // Remove one
        store
            .remove_decision_affects(decision.id, "function", "connect_db")
            .await
            .unwrap();
        let after_remove = store.list_decision_affects(decision.id).await.unwrap();
        assert_eq!(after_remove.len(), 1);
        assert_eq!(after_remove[0].entity_type, "file");
    }

    #[tokio::test]
    async fn test_supersede_decision() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Supersede task");
        store.create_task(plan.id, &task).await.unwrap();

        let old_decision = test_decision();
        store.create_decision(task.id, &old_decision).await.unwrap();

        let mut new_decision = test_decision();
        new_decision.description = "Use PostgreSQL + AGE".to_string();
        store.create_decision(task.id, &new_decision).await.unwrap();

        store
            .supersede_decision(new_decision.id, old_decision.id)
            .await
            .unwrap();

        // Old decision should now be superseded
        let old = store.get_decision(old_decision.id).await.unwrap().unwrap();
        assert_eq!(old.status, DecisionStatus::Superseded);

        // New decision should still be proposed
        let new = store.get_decision(new_decision.id).await.unwrap().unwrap();
        assert_eq!(new.status, DecisionStatus::Proposed);
    }

    #[tokio::test]
    async fn test_get_decisions_for_entity() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Entity decisions task");
        store.create_task(plan.id, &task).await.unwrap();

        let d1 = test_decision();
        store.create_decision(task.id, &d1).await.unwrap();
        store
            .add_decision_affects(d1.id, "file", "/src/db.rs", None)
            .await
            .unwrap();

        let mut d2 = test_decision();
        d2.description = "Use async runtime".to_string();
        store.create_decision(task.id, &d2).await.unwrap();
        store
            .add_decision_affects(d2.id, "file", "/src/db.rs", None)
            .await
            .unwrap();

        let decisions = store
            .get_decisions_for_entity("file", "/src/db.rs", 10)
            .await
            .unwrap();
        assert_eq!(decisions.len(), 2);
    }

    #[tokio::test]
    async fn test_decision_embedding() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Embedding task");
        store.create_task(plan.id, &task).await.unwrap();

        let decision = test_decision();
        store.create_decision(task.id, &decision).await.unwrap();

        // Initially no embedding
        let emb = store.get_decision_embedding(decision.id).await.unwrap();
        assert!(emb.is_none());

        // Set embedding
        let embedding: Vec<f32> = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        store
            .set_decision_embedding(decision.id, &embedding, "test-model")
            .await
            .unwrap();

        // Get embedding
        let retrieved = store
            .get_decision_embedding(decision.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.len(), 5);
        assert!((retrieved[0] - 0.1).abs() < 0.001);
        assert!((retrieved[4] - 0.5).abs() < 0.001);

        // Verify model was saved
        let d = store.get_decision(decision.id).await.unwrap().unwrap();
        assert_eq!(d.embedding_model, Some("test-model".to_string()));

        // Should not appear in "without embedding" list
        let without = store.get_decisions_without_embedding().await.unwrap();
        assert!(without.iter().all(|(id, _, _)| *id != decision.id));
    }

    #[tokio::test]
    async fn test_get_all_decisions_with_task_id() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        let t1 = test_task("Task A");
        let t2 = test_task("Task B");
        store.create_task(plan.id, &t1).await.unwrap();
        store.create_task(plan.id, &t2).await.unwrap();

        let d1 = test_decision();
        let d2 = test_decision();
        store.create_decision(t1.id, &d1).await.unwrap();
        store.create_decision(t2.id, &d2).await.unwrap();

        let all = store.get_all_decisions_with_task_id().await.unwrap();
        assert_eq!(all.len(), 2);

        let task_ids: Vec<Uuid> = all.iter().map(|(_, tid)| *tid).collect();
        assert!(task_ids.contains(&t1.id));
        assert!(task_ids.contains(&t2.id));
    }

    #[tokio::test]
    async fn test_decision_timeline() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Timeline task");
        store.create_task(plan.id, &task).await.unwrap();

        let d1 = test_decision();
        store.create_decision(task.id, &d1).await.unwrap();

        let mut d2 = test_decision();
        d2.description = "Revised approach".to_string();
        store.create_decision(task.id, &d2).await.unwrap();
        store.supersede_decision(d2.id, d1.id).await.unwrap();

        // Get timeline for the task
        let timeline = store
            .get_decision_timeline(Some(task.id), None, None)
            .await
            .unwrap();
        assert_eq!(timeline.len(), 2);

        // The old decision should show superseded_by
        let old_entry = timeline.iter().find(|e| e.decision.id == d1.id).unwrap();
        assert_eq!(old_entry.superseded_by, Some(d2.id));

        // The new decision should show supersedes_chain
        let new_entry = timeline.iter().find(|e| e.decision.id == d2.id).unwrap();
        assert!(new_entry.supersedes_chain.contains(&d1.id));

        // Test with date filter (all decisions should be recent)
        let from = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        let to = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let filtered = store
            .get_decision_timeline(None, Some(&from), Some(&to))
            .await
            .unwrap();
        assert_eq!(filtered.len(), 2);
    }

    #[tokio::test]
    async fn test_search_decisions_fts_project_filter() {
        let store = setup().await;
        let project_a = test_project_named("Decision Project A");
        let project_b = test_project_named("Decision Project B");
        store.create_project(&project_a).await.unwrap();
        store.create_project(&project_b).await.unwrap();

        let mut plan_a = test_plan();
        plan_a.project_id = Some(project_a.id);
        let mut plan_b = test_plan();
        plan_b.project_id = Some(project_b.id);
        store.create_plan(&plan_a).await.unwrap();
        store.create_plan(&plan_b).await.unwrap();

        let task_a = test_task("Task A");
        let task_b = test_task("Task B");
        store.create_task(plan_a.id, &task_a).await.unwrap();
        store.create_task(plan_b.id, &task_b).await.unwrap();

        let mut decision_a = test_decision();
        decision_a.description = "Use event bus for domain events".to_string();
        let mut decision_b = test_decision();
        decision_b.description = "Use event bus for domain events".to_string();
        store.create_decision(task_a.id, &decision_a).await.unwrap();
        store.create_decision(task_b.id, &decision_b).await.unwrap();

        let filtered = store
            .search_decisions_fts("event bus", 10, Some(&project_a.id.to_string()))
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0.id, decision_a.id);
    }
}
