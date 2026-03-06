//! Feature graph CRUD operations for IndentiaGraphStore.
//!
//! Covers: feature graph creation, retrieval, detail views, entity management,
//! auto-build from call graph traversal, and refresh.

use anyhow::{Context, Result};
use chrono::Utc;
use cortex_core::models::{
    FeatureGraphDetail, FeatureGraphEntity, FeatureGraphNode, FeatureGraphRelation,
};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

// ============================================================================
// SurrealDB record types
// ============================================================================

#[derive(Debug, SurrealValue)]
struct FeatureGraphRecord {
    id: RecordId,
    name: String,
    description: Option<String>,
    project_id: String,
    created_at: String,
    updated_at: Option<String>,
    entry_function: Option<String>,
    build_depth: Option<i64>,
    include_relations: Option<String>,
}

impl FeatureGraphRecord {
    fn into_node(self) -> Result<FeatureGraphNode> {
        Ok(FeatureGraphNode {
            id: rid_to_uuid(&self.id)?,
            name: self.name,
            description: self.description,
            project_id: Uuid::parse_str(&self.project_id).unwrap_or_else(|_| Uuid::nil()),
            created_at: self.created_at.parse().unwrap_or_else(|_| Utc::now()),
            updated_at: self
                .updated_at
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(Utc::now),
            entity_count: None,
            entry_function: self.entry_function,
            build_depth: self.build_depth.map(|v| v as u32),
            include_relations: self
                .include_relations
                .and_then(|s| serde_json::from_str(&s).ok()),
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, SurrealValue)]
struct FeatureEntityRecord {
    entity_type: Option<String>,
    entity_id: String,
    name: Option<String>,
    role: Option<String>,
}

impl IndentiaGraphStore {
    /// Create a feature graph.
    pub async fn create_feature_graph(&self, graph: &FeatureGraphNode) -> Result<()> {
        let rid = RecordId::new("feature_graph", graph.id.to_string().as_str());
        let relations_json = graph
            .include_relations
            .as_ref()
            .map(|r| serde_json::to_string(r).unwrap_or_default());

        self.db
            .query(
                "CREATE $rid SET \
                 name = $name, description = $desc, project_id = $pid, \
                 created_at = $created_at, updated_at = $updated_at, \
                 entry_function = $ef, build_depth = $bd, \
                 include_relations = $ir \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("name", graph.name.clone()))
            .bind(("desc", graph.description.clone()))
            .bind(("pid", graph.project_id.to_string()))
            .bind(("created_at", graph.created_at.to_rfc3339()))
            .bind(("updated_at", graph.updated_at.to_rfc3339()))
            .bind(("ef", graph.entry_function.clone()))
            .bind(("bd", graph.build_depth.map(|v| v as i64)))
            .bind(("ir", relations_json))
            .await
            .context("Failed to create feature graph")?;
        Ok(())
    }

    /// Get a feature graph by ID.
    pub async fn get_feature_graph(&self, id: Uuid) -> Result<Option<FeatureGraphNode>> {
        let rid = RecordId::new("feature_graph", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get feature graph")?;
        let records: Vec<FeatureGraphRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => {
                let mut node = r.into_node()?;
                // Count entities
                let count_q = format!(
                    "SELECT count() AS count FROM part_of_feature WHERE out = type::record('feature_graph', '{}') GROUP ALL",
                    id
                );
                let mut count_resp = self.db.query(&count_q).await?;
                let counts: Vec<serde_json::Value> = count_resp.take(0)?;
                node.entity_count = counts
                    .first()
                    .and_then(|v| v.get("count"))
                    .and_then(|v| v.as_i64());
                Ok(Some(node))
            }
            None => Ok(None),
        }
    }

    /// Get feature graph with full detail (entities + relations).
    pub async fn get_feature_graph_detail(&self, id: Uuid) -> Result<Option<FeatureGraphDetail>> {
        let graph = match self.get_feature_graph(id).await? {
            Some(g) => g,
            None => return Ok(None),
        };

        // Get entities — cast record IDs to string to avoid SurrealDB record type issues
        let entity_q = format!(
            "SELECT string::concat(record::tb(in), ':', record::id(in)) AS entity_rid, role \
             FROM part_of_feature \
             WHERE out = type::record('feature_graph', '{}')",
            id
        );
        let mut entity_resp = self.db.query(&entity_q).await?;
        let entity_raw: Vec<serde_json::Value> = entity_resp.take(0)?;

        let mut entities = Vec::new();
        for v in &entity_raw {
            let entity_rid = v.get("entity_rid").and_then(|v| v.as_str()).unwrap_or("");
            let role = v
                .get("role")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Parse entity type from record ID (e.g., "function:key" -> "function")
            let parts: Vec<&str> = entity_rid.split(':').collect();
            let entity_type = parts.first().unwrap_or(&"unknown").to_string();
            let entity_id = if parts.len() > 1 {
                parts[1..].join(":")
            } else {
                entity_rid.to_string()
            };

            entities.push(FeatureGraphEntity {
                entity_type: entity_type.clone(),
                entity_id: entity_id.clone(),
                name: None,
                role,
                importance_score: None,
            });
        }

        // Get relations between entities in this feature graph
        let relations = self.get_feature_graph_relations(id, &entities).await?;

        Ok(Some(FeatureGraphDetail {
            graph,
            entities,
            relations,
        }))
    }

    /// Get relations between entities in a feature graph.
    async fn get_feature_graph_relations(
        &self,
        _id: Uuid,
        entities: &[FeatureGraphEntity],
    ) -> Result<Vec<FeatureGraphRelation>> {
        let mut relations = Vec::new();

        // For each function entity, check calls to other entities
        let func_entities: Vec<&FeatureGraphEntity> = entities
            .iter()
            .filter(|e| e.entity_type == "function")
            .collect();

        for entity in &func_entities {
            let func_rid = RecordId::new("function", entity.entity_id.as_str());
            let mut resp = self
                .db
                .query(
                    "SELECT out.name AS target_name, out.file_path AS target_file \
                     FROM calls WHERE in = $func_rid LIMIT 100",
                )
                .bind(("func_rid", func_rid))
                .await?;
            let records: Vec<serde_json::Value> = resp.take(0)?;

            for rec in records {
                let target_name = rec
                    .get("target_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // Check if target is in our entity set
                if entities.iter().any(|e| e.entity_id.contains(target_name)) {
                    relations.push(FeatureGraphRelation {
                        source_type: entity.entity_type.clone(),
                        source_id: entity.entity_id.clone(),
                        target_type: "function".to_string(),
                        target_id: target_name.to_string(),
                        relation_type: "CALLS".to_string(),
                    });
                }
            }
        }

        Ok(relations)
    }

    /// List feature graphs, optionally filtered by project.
    pub async fn list_feature_graphs(
        &self,
        project_id: Option<Uuid>,
    ) -> Result<Vec<FeatureGraphNode>> {
        let mut resp = if let Some(pid) = project_id {
            self.db
                .query(
                    "SELECT * FROM feature_graph WHERE project_id = $pid ORDER BY created_at DESC",
                )
                .bind(("pid", pid.to_string()))
                .await
                .context("list_feature_graphs")?
        } else {
            self.db
                .query("SELECT * FROM feature_graph ORDER BY created_at DESC")
                .await
                .context("list_feature_graphs")?
        };
        let records: Vec<FeatureGraphRecord> = resp.take(0)?;
        let mut results = Vec::new();
        for r in records {
            if let Ok(mut node) = r.into_node() {
                // Count entities for each graph
                let fg_rid = RecordId::new("feature_graph", node.id.to_string().as_str());
                let mut count_resp = self
                    .db
                    .query(
                        "SELECT count() AS count FROM part_of_feature \
                         WHERE out = $fg_rid GROUP ALL",
                    )
                    .bind(("fg_rid", fg_rid))
                    .await?;
                let counts: Vec<serde_json::Value> = count_resp.take(0)?;
                node.entity_count = counts
                    .first()
                    .and_then(|v| v.get("count"))
                    .and_then(|v| v.as_i64());
                results.push(node);
            }
        }
        Ok(results)
    }

    /// Delete a feature graph and its entity associations.
    pub async fn delete_feature_graph(&self, id: Uuid) -> Result<bool> {
        let id_str = id.to_string();
        let rid = RecordId::new("feature_graph", id_str.as_str());

        // Check existence first
        let mut check = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid.clone()))
            .await?;
        let existing: Vec<FeatureGraphRecord> = check.take(0)?;
        if existing.is_empty() {
            return Ok(false);
        }

        // Delete edges first, then the node
        self.db
            .query(
                "DELETE FROM part_of_feature WHERE out = $rid; \
                 DELETE $rid",
            )
            .bind(("rid", rid))
            .await
            .context("Failed to delete feature graph")?;
        Ok(true)
    }

    /// Add an entity to a feature graph.
    pub async fn add_entity_to_feature_graph(
        &self,
        feature_graph_id: Uuid,
        entity_type: &str,
        entity_id: &str,
        role: Option<&str>,
        _project_id: Option<Uuid>,
    ) -> Result<()> {
        let fg_rid = RecordId::new("feature_graph", feature_graph_id.to_string().as_str());

        // Construct the entity record ID
        let entity_table = match entity_type.to_lowercase().as_str() {
            "function" => "function",
            "struct" => "struct",
            "trait" => "trait",
            "enum" => "enum",
            "file" => "file",
            "impl" => "impl",
            _ => "file", // fallback
        };
        let entity_rid = RecordId::new(entity_table, entity_id);

        self.db
            .query("RELATE $entity->part_of_feature->$fg SET role = $role RETURN NONE")
            .bind(("entity", entity_rid))
            .bind(("fg", fg_rid))
            .bind(("role", role.map(|r| r.to_string())))
            .await
            .context("Failed to add entity to feature graph")?;

        // Update the feature graph timestamp
        let update_q = format!(
            "UPDATE feature_graph SET updated_at = '{}' WHERE id = type::record('feature_graph', '{}') RETURN NONE",
            Utc::now().to_rfc3339(),
            feature_graph_id
        );
        self.db.query(&update_q).await?;

        Ok(())
    }

    /// Remove an entity from a feature graph.
    pub async fn remove_entity_from_feature_graph(
        &self,
        feature_graph_id: Uuid,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<bool> {
        let entity_table = match entity_type.to_lowercase().as_str() {
            "function" => "function",
            "struct" => "struct",
            "trait" => "trait",
            "enum" => "enum",
            "file" => "file",
            "impl" => "impl",
            _ => "file",
        };

        let entity_rid = RecordId::new(entity_table, entity_id);
        let fg_rid = RecordId::new("feature_graph", feature_graph_id.to_string().as_str());
        self.db
            .query("DELETE FROM part_of_feature WHERE in = $entity_rid AND out = $fg_rid")
            .bind(("entity_rid", entity_rid))
            .bind(("fg_rid", fg_rid))
            .await
            .context("Failed to remove entity from feature graph")?;

        Ok(true) // SurrealDB DELETE is idempotent; assume success
    }

    /// Auto-build a feature graph by BFS from an entry function.
    pub async fn auto_build_feature_graph(
        &self,
        name: &str,
        description: Option<&str>,
        project_id: Uuid,
        entry_function: &str,
        depth: u32,
        include_relations: Option<&[String]>,
        _filter_community: Option<bool>,
    ) -> Result<FeatureGraphDetail> {
        let now = Utc::now();
        let fg_id = Uuid::new_v4();

        let graph = FeatureGraphNode {
            id: fg_id,
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            project_id,
            created_at: now,
            updated_at: now,
            entity_count: None,
            entry_function: Some(entry_function.to_string()),
            build_depth: Some(depth),
            include_relations: include_relations.map(|r| r.to_vec()),
        };
        self.create_feature_graph(&graph).await?;

        // BFS through calls from entry function
        // Find entry function
        let mut resp = self
            .db
            .query(
                "SELECT id, name, file_path FROM `function` \
                 WHERE name = $entry_fn \
                 AND file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
                 LIMIT 1",
            )
            .bind(("entry_fn", entry_function.to_string()))
            .bind(("pid", project_id.to_string()))
            .await?;
        let entries: Vec<serde_json::Value> = resp.take(0)?;

        let mut entities = Vec::new();
        let mut seen = std::collections::HashSet::new();

        if let Some(entry) = entries.first() {
            let entry_id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let entry_name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let entry_file = entry
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Add entry point
            seen.insert(entry_id.to_string());
            self.add_entity_to_feature_graph(
                fg_id,
                "function",
                entry_id,
                Some("entry_point"),
                Some(project_id),
            )
            .await?;
            entities.push(FeatureGraphEntity {
                entity_type: "function".to_string(),
                entity_id: entry_id.to_string(),
                name: Some(entry_name.to_string()),
                role: Some("entry_point".to_string()),
                importance_score: None,
            });

            // Add entry file
            if !entry_file.is_empty() && seen.insert(entry_file.to_string()) {
                self.add_entity_to_feature_graph(
                    fg_id,
                    "file",
                    entry_file,
                    Some("support"),
                    Some(project_id),
                )
                .await?;
                entities.push(FeatureGraphEntity {
                    entity_type: "file".to_string(),
                    entity_id: entry_file.to_string(),
                    name: Some(entry_file.to_string()),
                    role: Some("support".to_string()),
                    importance_score: None,
                });
            }

            // BFS through callees
            let include_calls = include_relations
                .map(|r| r.iter().any(|s| s.to_uppercase() == "CALLS"))
                .unwrap_or(true);

            if include_calls {
                let mut frontier = vec![entry_id.to_string()];
                for d in 0..depth {
                    if frontier.is_empty() {
                        break;
                    }
                    let role = if d == 0 { "core_logic" } else { "support" };
                    let mut next = Vec::new();

                    for fid in &frontier {
                        let func_rid = RecordId::new("function", fid.as_str());
                        let mut r = self
                            .db
                            .query(
                                "SELECT id, name, file_path FROM `function` WHERE id IN \
                                 (SELECT VALUE out.id FROM calls WHERE in = $func_rid) LIMIT 100",
                            )
                            .bind(("func_rid", func_rid))
                            .await?;
                        let recs: Vec<serde_json::Value> = r.take(0)?;
                        for rec in recs {
                            let rid = rec.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            let rname = rec.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let rfile = rec.get("file_path").and_then(|v| v.as_str()).unwrap_or("");

                            if seen.insert(rid.to_string()) {
                                self.add_entity_to_feature_graph(
                                    fg_id,
                                    "function",
                                    rid,
                                    Some(role),
                                    Some(project_id),
                                )
                                .await?;
                                entities.push(FeatureGraphEntity {
                                    entity_type: "function".to_string(),
                                    entity_id: rid.to_string(),
                                    name: Some(rname.to_string()),
                                    role: Some(role.to_string()),
                                    importance_score: None,
                                });
                                next.push(rid.to_string());

                                // Add containing file
                                if !rfile.is_empty() && seen.insert(rfile.to_string()) {
                                    self.add_entity_to_feature_graph(
                                        fg_id,
                                        "file",
                                        rfile,
                                        Some("support"),
                                        Some(project_id),
                                    )
                                    .await?;
                                    entities.push(FeatureGraphEntity {
                                        entity_type: "file".to_string(),
                                        entity_id: rfile.to_string(),
                                        name: Some(rfile.to_string()),
                                        role: Some("support".to_string()),
                                        importance_score: None,
                                    });
                                }
                            }
                        }
                    }
                    frontier = next;
                }
            }
        }

        // Build relations
        let relations = self.get_feature_graph_relations(fg_id, &entities).await?;

        // Update entity count
        let mut final_graph = graph;
        final_graph.entity_count = Some(entities.len() as i64);

        Ok(FeatureGraphDetail {
            graph: final_graph,
            entities,
            relations,
        })
    }

    /// Refresh a feature graph by re-running auto_build with its stored parameters.
    pub async fn refresh_feature_graph(&self, id: Uuid) -> Result<Option<FeatureGraphDetail>> {
        let graph = match self.get_feature_graph(id).await? {
            Some(g) => g,
            None => return Ok(None),
        };

        let entry_function = match &graph.entry_function {
            Some(f) => f.clone(),
            None => return self.get_feature_graph_detail(id).await,
        };

        let depth = graph.build_depth.unwrap_or(2);

        // Delete existing entity associations
        let del_q = format!(
            "DELETE FROM part_of_feature WHERE out = type::record('feature_graph', '{}')",
            id
        );
        self.db.query(&del_q).await?;

        // Re-build entities via BFS (reuse auto_build logic but with existing graph)
        let include_relations_owned = graph.include_relations.clone();
        let include_refs: Option<Vec<String>> = include_relations_owned;
        let include_slice: Option<&[String]> = include_refs.as_deref();

        // Delete the graph and recreate
        self.delete_feature_graph(id).await?;

        let detail = self
            .auto_build_feature_graph(
                &graph.name,
                graph.description.as_deref(),
                graph.project_id,
                &entry_function,
                depth,
                include_slice,
                None,
            )
            .await?;

        Ok(Some(detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::test_helpers::*;

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_create_and_get_feature_graph() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let fg = FeatureGraphNode {
            id: Uuid::new_v4(),
            name: "Auth Flow".to_string(),
            description: Some("Authentication feature".to_string()),
            project_id: project.id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            entity_count: None,
            entry_function: Some("handle_login".to_string()),
            build_depth: Some(3),
            include_relations: Some(vec!["CALLS".to_string()]),
        };
        store.create_feature_graph(&fg).await.unwrap();

        let retrieved = store.get_feature_graph(fg.id).await.unwrap().unwrap();
        assert_eq!(retrieved.name, "Auth Flow");
        assert_eq!(retrieved.entry_function, Some("handle_login".to_string()));
    }

    #[tokio::test]
    async fn test_list_feature_graphs() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let fg1 = FeatureGraphNode {
            id: Uuid::new_v4(),
            name: "Feature A".to_string(),
            description: None,
            project_id: project.id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            entity_count: None,
            entry_function: None,
            build_depth: None,
            include_relations: None,
        };
        let fg2 = FeatureGraphNode {
            id: Uuid::new_v4(),
            name: "Feature B".to_string(),
            description: None,
            project_id: project.id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            entity_count: None,
            entry_function: None,
            build_depth: None,
            include_relations: None,
        };
        store.create_feature_graph(&fg1).await.unwrap();
        store.create_feature_graph(&fg2).await.unwrap();

        let all = store.list_feature_graphs(None).await.unwrap();
        assert_eq!(all.len(), 2);

        let filtered = store.list_feature_graphs(Some(project.id)).await.unwrap();
        assert_eq!(filtered.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_feature_graph() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let fg = FeatureGraphNode {
            id: Uuid::new_v4(),
            name: "To Delete".to_string(),
            description: None,
            project_id: project.id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            entity_count: None,
            entry_function: None,
            build_depth: None,
            include_relations: None,
        };
        store.create_feature_graph(&fg).await.unwrap();
        assert!(store.delete_feature_graph(fg.id).await.unwrap());
        assert!(store.get_feature_graph(fg.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_add_entity_to_feature_graph() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let file = test_file("src/main.rs");
        store.upsert_file(&file).await.unwrap();

        let fg = FeatureGraphNode {
            id: Uuid::new_v4(),
            name: "Entity Test".to_string(),
            description: None,
            project_id: project.id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            entity_count: None,
            entry_function: None,
            build_depth: None,
            include_relations: None,
        };
        store.create_feature_graph(&fg).await.unwrap();

        store
            .add_entity_to_feature_graph(
                fg.id,
                "file",
                "src/main.rs",
                Some("support"),
                Some(project.id),
            )
            .await
            .unwrap();

        let detail = store
            .get_feature_graph_detail(fg.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!detail.entities.is_empty());
    }

    #[tokio::test]
    async fn test_auto_build_feature_graph_no_entry() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        // Auto-build with a function that doesn't exist — should still create graph
        let detail = store
            .auto_build_feature_graph(
                "Empty Feature",
                Some("No functions"),
                project.id,
                "nonexistent_fn",
                2,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(detail.graph.name, "Empty Feature");
        assert!(detail.entities.is_empty());
    }

    #[tokio::test]
    async fn test_get_feature_graph_nonexistent() {
        let store = setup().await;
        let result = store.get_feature_graph(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }
}
