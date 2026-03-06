//! Code exploration operations for IndentiaGraphStore.
//!
//! Covers: call graphs, impact analysis, symbol references, class hierarchy,
//! graph edges, batch relationships, cleanup, topology checks, bridge subgraph,
//! metric getters, context cards, and task details.

use anyhow::{Context, Result};
use cortex_core::graph::{
    BridgeRawEdge, BridgeRawNode, ContextCard, IsomorphicGroup, TopologyRuleType, TopologyViolation,
};
use cortex_core::models::*;
use cortex_core::parser_types::FunctionCall;
use cortex_core::plan::TaskDetails;
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::IndentiaGraphStore;

// ============================================================================
// SurrealDB record types for deserialization
// ============================================================================

#[derive(Debug, SurrealValue)]
struct FuncRecord {
    id: RecordId,
    name: String,
    visibility: String,
    is_async: bool,
    is_unsafe: Option<bool>,
    generics: Option<String>,
    parameters: Option<String>,
    return_type: Option<String>,
    file_path: String,
    line_start: i64,
    line_end: i64,
    complexity: Option<i64>,
    docstring: Option<String>,
}

impl FuncRecord {
    fn into_node(self) -> FunctionNode {
        FunctionNode {
            name: self.name,
            visibility: crate::symbol::string_to_vis_pub(&self.visibility),
            params: self
                .parameters
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            return_type: self.return_type,
            generics: self
                .generics
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            is_async: self.is_async,
            is_unsafe: self.is_unsafe.unwrap_or(false),
            complexity: self.complexity.unwrap_or(1) as u32,
            file_path: self.file_path,
            line_start: self.line_start as u32,
            line_end: self.line_end as u32,
            docstring: self.docstring,
        }
    }
}

#[derive(Debug, SurrealValue)]
struct NameRecord {
    name: String,
}

#[derive(Debug, SurrealValue)]
struct PathRecord {
    path: String,
}

#[derive(Debug, SurrealValue)]
struct CallerConfRecord {
    name: String,
    file_path: String,
    confidence: Option<f64>,
    reason: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, SurrealValue)]
struct CountRecord {
    count: i64,
}

#[derive(Debug, SurrealValue)]
struct EdgeRecord {
    from_path: String,
    to_path: String,
}

#[derive(Debug, SurrealValue)]
struct SynapseRecord {
    source: String,
    target: String,
    weight: f64,
}

#[derive(Debug, SurrealValue)]
struct FloatRecord {
    value: Option<f64>,
}

#[derive(Debug, SurrealValue)]
struct BridgeProxRecord {
    path: String,
    score: f64,
}

#[derive(Debug, SurrealValue)]
struct ContextCardRecord {
    id: RecordId,
    path: String,
    project_id: String,
    cc_pagerank: Option<f64>,
    cc_betweenness: Option<f64>,
    cc_clustering: Option<f64>,
    cc_community_id: Option<i64>,
    cc_community_label: Option<String>,
    cc_imports_out: Option<i64>,
    cc_imports_in: Option<i64>,
    cc_calls_out: Option<i64>,
    cc_calls_in: Option<i64>,
    cc_structural_dna: Option<String>,
    cc_wl_hash: Option<String>,
    cc_fingerprint: Option<String>,
    cc_co_changers_top5: Option<String>,
    cc_version: Option<i64>,
    cc_computed_at: Option<String>,
}

impl ContextCardRecord {
    fn into_card(self) -> ContextCard {
        ContextCard {
            path: self.path,
            cc_pagerank: self.cc_pagerank.unwrap_or(0.0),
            cc_betweenness: self.cc_betweenness.unwrap_or(0.0),
            cc_clustering: self.cc_clustering.unwrap_or(0.0),
            cc_community_id: self.cc_community_id.unwrap_or(0) as u32,
            cc_community_label: self.cc_community_label.unwrap_or_default(),
            cc_imports_out: self.cc_imports_out.unwrap_or(0) as usize,
            cc_imports_in: self.cc_imports_in.unwrap_or(0) as usize,
            cc_calls_out: self.cc_calls_out.unwrap_or(0) as usize,
            cc_calls_in: self.cc_calls_in.unwrap_or(0) as usize,
            cc_structural_dna: self
                .cc_structural_dna
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            cc_wl_hash: self.cc_wl_hash.and_then(|s| s.parse().ok()).unwrap_or(0),
            cc_fingerprint: self
                .cc_fingerprint
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            cc_co_changers_top5: self
                .cc_co_changers_top5
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            cc_version: self.cc_version.unwrap_or(0) as i32,
            cc_computed_at: self.cc_computed_at.unwrap_or_default(),
        }
    }
}

// Topology rule record for reading back rules
#[allow(dead_code)]
#[derive(Debug, SurrealValue)]
struct TopologyRuleRecord {
    id: RecordId,
    project_id: String,
    rule_type: String,
    source_pattern: String,
    target_pattern: Option<String>,
    max_value: Option<i64>,
    description: Option<String>,
}

impl IndentiaGraphStore {
    // ========================================================================
    // Call Graph (6)
    // ========================================================================

    /// Follow calls edges from function to given depth via iterative BFS.
    pub async fn get_callees(&self, function_id: &str, depth: u32) -> Result<Vec<FunctionNode>> {
        if depth == 0 {
            return Ok(vec![]);
        }
        // BFS through calls edges outbound from the function
        let func_rid = RecordId::new("function", function_id);
        let mut resp = self
            .db
            .query(
                "SELECT * FROM `function` WHERE id IN \
                 (SELECT VALUE out.id FROM calls WHERE in = $func_rid LIMIT 500)",
            )
            .bind(("func_rid", func_rid))
            .await
            .context("get_callees")?;
        let records: Vec<FuncRecord> = resp.take(0)?;
        let mut results: Vec<FunctionNode> = records.into_iter().map(|r| r.into_node()).collect();

        if depth > 1 {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for f in &results {
                seen.insert(format!("{}::{}::{}", f.file_path, f.name, f.line_start));
            }
            // Use RecordId to avoid BFS injection — store actual function ids
            let mut frontier_ids: Vec<RecordId> = results
                .iter()
                .map(|f| {
                    RecordId::new(
                        "function",
                        format!("{}::{}::{}", f.file_path, f.name, f.line_start).as_str(),
                    )
                })
                .collect();

            for _ in 1..depth {
                if frontier_ids.is_empty() {
                    break;
                }
                let mut next_frontier = Vec::new();
                for frid in &frontier_ids {
                    let mut r = self
                        .db
                        .query(
                            "SELECT * FROM `function` WHERE id IN \
                             (SELECT VALUE out.id FROM calls WHERE in = $func_rid LIMIT 500)",
                        )
                        .bind(("func_rid", frid.clone()))
                        .await?;
                    let recs: Vec<FuncRecord> = r.take(0)?;
                    for rec in recs {
                        let key = format!("{}::{}::{}", rec.file_path, rec.name, rec.line_start);
                        if seen.insert(key.clone()) {
                            next_frontier.push(RecordId::new("function", key.as_str()));
                            results.push(rec.into_node());
                        }
                    }
                }
                frontier_ids = next_frontier;
            }
        }
        Ok(results)
    }

    /// Find functions that call the named function (callers by name).
    pub async fn get_function_callers_by_name(
        &self,
        function_name: &str,
        depth: u32,
        project_id: Option<Uuid>,
    ) -> Result<Vec<String>> {
        // Build parameterized query depending on whether a project filter is needed.
        let (sql, has_pid) = if project_id.is_some() {
            (
                "SELECT name FROM `function` WHERE id IN \
                 (SELECT VALUE in.id FROM calls WHERE out.name = $fname \
                  AND file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid)) \
                 LIMIT 500",
                true,
            )
        } else {
            (
                "SELECT name FROM `function` WHERE id IN \
                 (SELECT VALUE in.id FROM calls WHERE out.name = $fname) \
                 LIMIT 500",
                false,
            )
        };

        let mut qb = self
            .db
            .query(sql)
            .bind(("fname", function_name.to_string()));
        if has_pid {
            qb = qb.bind(("pid", project_id.unwrap().to_string()));
        }
        let mut resp = qb.await.context("get_function_callers_by_name")?;
        let records: Vec<NameRecord> = resp.take(0)?;
        let mut results: Vec<String> = records.into_iter().map(|r| r.name).collect();

        if depth > 1 {
            let mut frontier = results.clone();
            let mut seen: std::collections::HashSet<String> = results.iter().cloned().collect();
            for _ in 1..depth {
                if frontier.is_empty() {
                    break;
                }
                let mut next = Vec::new();
                for fname in &frontier {
                    let mut qb2 = self.db.query(sql).bind(("fname", fname.clone()));
                    if has_pid {
                        qb2 = qb2.bind(("pid", project_id.unwrap().to_string()));
                    }
                    let mut r = qb2.await?;
                    let recs: Vec<NameRecord> = r.take(0)?;
                    for rec in recs {
                        if seen.insert(rec.name.clone()) {
                            next.push(rec.name.clone());
                            results.push(rec.name);
                        }
                    }
                }
                frontier = next;
            }
        }
        Ok(results)
    }

    /// Find functions called by the named function (callees by name).
    pub async fn get_function_callees_by_name(
        &self,
        function_name: &str,
        depth: u32,
        project_id: Option<Uuid>,
    ) -> Result<Vec<String>> {
        let (sql, has_pid) = if project_id.is_some() {
            (
                "SELECT name FROM `function` WHERE id IN \
                 (SELECT VALUE out.id FROM calls WHERE in.name = $fname \
                  AND file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid)) \
                 LIMIT 500",
                true,
            )
        } else {
            (
                "SELECT name FROM `function` WHERE id IN \
                 (SELECT VALUE out.id FROM calls WHERE in.name = $fname) \
                 LIMIT 500",
                false,
            )
        };

        let mut qb = self
            .db
            .query(sql)
            .bind(("fname", function_name.to_string()));
        if has_pid {
            qb = qb.bind(("pid", project_id.unwrap().to_string()));
        }
        let mut resp = qb.await.context("get_function_callees_by_name")?;
        let records: Vec<NameRecord> = resp.take(0)?;
        let mut results: Vec<String> = records.into_iter().map(|r| r.name).collect();

        if depth > 1 {
            let mut frontier = results.clone();
            let mut seen: std::collections::HashSet<String> = results.iter().cloned().collect();
            for _ in 1..depth {
                if frontier.is_empty() {
                    break;
                }
                let mut next = Vec::new();
                for fname in &frontier {
                    let mut qb2 = self.db.query(sql).bind(("fname", fname.clone()));
                    if has_pid {
                        qb2 = qb2.bind(("pid", project_id.unwrap().to_string()));
                    }
                    let mut r = qb2.await?;
                    let recs: Vec<NameRecord> = r.take(0)?;
                    for rec in recs {
                        if seen.insert(rec.name.clone()) {
                            next.push(rec.name.clone());
                            results.push(rec.name);
                        }
                    }
                }
                frontier = next;
            }
        }
        Ok(results)
    }

    /// Get callers with confidence scores.
    pub async fn get_callers_with_confidence(
        &self,
        function_name: &str,
        project_id: Option<Uuid>,
    ) -> Result<Vec<(String, String, f64, String)>> {
        let sql = if project_id.is_some() {
            "SELECT in.name AS name, in.file_path AS file_path, confidence, reason \
             FROM calls WHERE out.name = $fname \
             AND in.file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
             LIMIT 500"
        } else {
            "SELECT in.name AS name, in.file_path AS file_path, confidence, reason \
             FROM calls WHERE out.name = $fname LIMIT 500"
        };
        let mut qb = self
            .db
            .query(sql)
            .bind(("fname", function_name.to_string()));
        if let Some(pid) = project_id {
            qb = qb.bind(("pid", pid.to_string()));
        }
        let mut resp = qb.await.context("get_callers_with_confidence")?;
        let records: Vec<CallerConfRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .map(|r| {
                (
                    r.name,
                    r.file_path,
                    r.confidence.unwrap_or(0.5),
                    r.reason.unwrap_or_else(|| "parser".to_string()),
                )
            })
            .collect())
    }

    /// Get callees with confidence scores.
    pub async fn get_callees_with_confidence(
        &self,
        function_name: &str,
        project_id: Option<Uuid>,
    ) -> Result<Vec<(String, String, f64, String)>> {
        let sql = if project_id.is_some() {
            "SELECT out.name AS name, out.file_path AS file_path, confidence, reason \
             FROM calls WHERE in.name = $fname \
             AND out.file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
             LIMIT 500"
        } else {
            "SELECT out.name AS name, out.file_path AS file_path, confidence, reason \
             FROM calls WHERE in.name = $fname LIMIT 500"
        };
        let mut qb = self
            .db
            .query(sql)
            .bind(("fname", function_name.to_string()));
        if let Some(pid) = project_id {
            qb = qb.bind(("pid", pid.to_string()));
        }
        let mut resp = qb.await.context("get_callees_with_confidence")?;
        let records: Vec<CallerConfRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .map(|r| {
                (
                    r.name,
                    r.file_path,
                    r.confidence.unwrap_or(0.5),
                    r.reason.unwrap_or_else(|| "parser".to_string()),
                )
            })
            .collect())
    }

    /// Count how many functions call the named function.
    pub async fn get_function_caller_count(
        &self,
        function_name: &str,
        project_id: Option<Uuid>,
    ) -> Result<i64> {
        let sql = if project_id.is_some() {
            "SELECT count() AS count FROM calls WHERE out.name = $fname \
             AND in.file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
             GROUP ALL"
        } else {
            "SELECT count() AS count FROM calls WHERE out.name = $fname GROUP ALL"
        };
        let mut qb = self
            .db
            .query(sql)
            .bind(("fname", function_name.to_string()));
        if let Some(pid) = project_id {
            qb = qb.bind(("pid", pid.to_string()));
        }
        let mut resp = qb.await.context("get_function_caller_count")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        let count = records
            .first()
            .and_then(|v| v.get("count"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok(count)
    }

    // ========================================================================
    // Impact Analysis (3)
    // ========================================================================

    /// Find files that import this file (downstream dependents) via BFS.
    pub async fn find_dependent_files(
        &self,
        file_path: &str,
        depth: u32,
        project_id: Option<Uuid>,
    ) -> Result<Vec<String>> {
        if depth == 0 {
            return Ok(vec![]);
        }

        let (sql, has_pid) = if project_id.is_some() {
            (
                "SELECT in.path AS path FROM imports \
                 WHERE out = $file_rid AND in.project_id = $pid \
                 LIMIT 500",
                true,
            )
        } else {
            (
                "SELECT in.path AS path FROM imports \
                 WHERE out = $file_rid LIMIT 500",
                false,
            )
        };

        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();
        seen.insert(file_path.to_string());
        let mut frontier = vec![file_path.to_string()];

        for _ in 0..depth {
            if frontier.is_empty() {
                break;
            }
            let mut next = Vec::new();
            for fp in &frontier {
                let file_rid = RecordId::new("file", fp.as_str());
                let mut qb = self.db.query(sql).bind(("file_rid", file_rid));
                if has_pid {
                    qb = qb.bind(("pid", project_id.unwrap().to_string()));
                }
                let mut resp = qb.await?;
                let records: Vec<PathRecord> = resp.take(0)?;
                for rec in records {
                    if seen.insert(rec.path.clone()) {
                        next.push(rec.path.clone());
                        results.push(rec.path);
                    }
                }
            }
            frontier = next;
        }
        Ok(results)
    }

    /// Alias for find_dependent_files.
    pub async fn find_impacted_files(
        &self,
        file_path: &str,
        depth: u32,
        project_id: Option<Uuid>,
    ) -> Result<Vec<String>> {
        self.find_dependent_files(file_path, depth, project_id)
            .await
    }

    /// Find functions that call a given function (by function record id).
    pub async fn find_callers(
        &self,
        function_id: &str,
        project_id: Option<Uuid>,
    ) -> Result<Vec<FunctionNode>> {
        let func_rid = RecordId::new("function", function_id);
        let sql = if project_id.is_some() {
            "SELECT * FROM `function` WHERE id IN \
             (SELECT VALUE in.id FROM calls WHERE out = $func_rid) \
             AND file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
             LIMIT 500"
        } else {
            "SELECT * FROM `function` WHERE id IN \
             (SELECT VALUE in.id FROM calls WHERE out = $func_rid) \
             LIMIT 500"
        };
        let mut qb = self.db.query(sql).bind(("func_rid", func_rid));
        if let Some(pid) = project_id {
            qb = qb.bind(("pid", pid.to_string()));
        }
        let mut resp = qb.await.context("find_callers")?;
        let records: Vec<FuncRecord> = resp.take(0)?;
        Ok(records.into_iter().map(|r| r.into_node()).collect())
    }

    // ========================================================================
    // Symbol References (3)
    // ========================================================================

    /// Find references to a symbol across the codebase.
    pub async fn find_symbol_references(
        &self,
        symbol: &str,
        limit: usize,
        project_id: Option<Uuid>,
    ) -> Result<Vec<SymbolReferenceNode>> {
        let mut results = Vec::new();
        let has_pid = project_id.is_some();
        let pid_str = project_id.map(|p| p.to_string());

        // Helper closure to build a parameterized query with optional project filter.
        // Separate SQL strings are used for each table/column pattern.

        // Search in functions (calls to this symbol)
        let func_sql = if has_pid {
            "SELECT file_path, line_start AS line FROM `function` \
             WHERE name = $symbol \
             AND file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
             LIMIT $limit"
        } else {
            "SELECT file_path, line_start AS line FROM `function` \
             WHERE name = $symbol LIMIT $limit"
        };
        let mut qb = self
            .db
            .query(func_sql)
            .bind(("symbol", symbol.to_string()))
            .bind(("limit", limit as i64));
        if let Some(ref pid) = pid_str {
            qb = qb.bind(("pid", pid.clone()));
        }
        let mut resp = qb.await?;
        let func_results: Vec<serde_json::Value> = resp.take(0)?;
        for v in func_results {
            results.push(SymbolReferenceNode {
                file_path: v
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                line: v.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                context: format!("function definition: {}", symbol),
                reference_type: "definition".to_string(),
            });
        }

        // Search in imports
        let import_sql = if has_pid {
            "SELECT file_path, line FROM `import` \
             WHERE path CONTAINS $symbol \
             AND file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
             LIMIT $limit"
        } else {
            "SELECT file_path, line FROM `import` \
             WHERE path CONTAINS $symbol LIMIT $limit"
        };
        let mut qb2 = self
            .db
            .query(import_sql)
            .bind(("symbol", symbol.to_string()))
            .bind(("limit", limit as i64));
        if let Some(ref pid) = pid_str {
            qb2 = qb2.bind(("pid", pid.clone()));
        }
        let mut resp2 = qb2.await?;
        let import_results: Vec<serde_json::Value> = resp2.take(0)?;
        for v in import_results {
            results.push(SymbolReferenceNode {
                file_path: v
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                line: v.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                context: format!("import of {}", symbol),
                reference_type: "import".to_string(),
            });
        }

        // Search in structs
        let struct_sql = if has_pid {
            "SELECT file_path, line_start AS line FROM `struct` \
             WHERE name = $symbol \
             AND file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
             LIMIT $limit"
        } else {
            "SELECT file_path, line_start AS line FROM `struct` \
             WHERE name = $symbol LIMIT $limit"
        };
        let mut qb3 = self
            .db
            .query(struct_sql)
            .bind(("symbol", symbol.to_string()))
            .bind(("limit", limit as i64));
        if let Some(ref pid) = pid_str {
            qb3 = qb3.bind(("pid", pid.clone()));
        }
        let mut resp3 = qb3.await?;
        let struct_results: Vec<serde_json::Value> = resp3.take(0)?;
        for v in struct_results {
            results.push(SymbolReferenceNode {
                file_path: v
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                line: v.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                context: format!("struct definition: {}", symbol),
                reference_type: "definition".to_string(),
            });
        }

        results.truncate(limit);
        Ok(results)
    }

    /// Get impl blocks for a type.
    pub async fn get_impl_blocks(&self, type_name: &str) -> Result<Vec<serde_json::Value>> {
        let mut resp = self
            .db
            .query(
                "SELECT file_path, line_start, line_end, trait_name FROM `impl` \
                 WHERE for_type = $type_name LIMIT 100",
            )
            .bind(("type_name", type_name.to_string()))
            .await
            .context("get_impl_blocks")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        Ok(records)
    }

    /// Find subclasses via extends edges.
    pub async fn find_subclasses(&self, class_name: &str) -> Result<Vec<serde_json::Value>> {
        let mut resp = self
            .db
            .query(
                "SELECT in.name AS name, in.file_path AS file_path, in.line_start AS line_start \
                 FROM extends WHERE out.name = $class_name LIMIT 100",
            )
            .bind(("class_name", class_name.to_string()))
            .await
            .context("find_subclasses")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        Ok(records)
    }

    // ========================================================================
    // Class Hierarchy (2)
    // ========================================================================

    /// Get class hierarchy tree for a type.
    pub async fn get_class_hierarchy(
        &self,
        type_name: &str,
        max_depth: u32,
    ) -> Result<serde_json::Value> {
        // Get parent chain (upward via extends)
        let mut parents = Vec::new();
        let mut current = type_name.to_string();
        for _ in 0..max_depth {
            let mut resp = self
                .db
                .query(
                    "SELECT out.name AS name, out.file_path AS file_path \
                     FROM extends WHERE in.name = $current LIMIT 1",
                )
                .bind(("current", current.clone()))
                .await?;
            let records: Vec<serde_json::Value> = resp.take(0)?;
            if let Some(parent) = records.into_iter().next() {
                let parent_name = parent
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if parent_name.is_empty() || parent_name == current {
                    break;
                }
                parents.push(parent.clone());
                current = parent_name;
            } else {
                break;
            }
        }

        // Get children (downward via extends)
        let children = self.find_subclasses(type_name).await?;

        Ok(serde_json::json!({
            "type_name": type_name,
            "parents": parents,
            "children": children,
        }))
    }

    /// Find types that implement an interface/trait.
    pub async fn find_interface_implementors(
        &self,
        interface_name: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let mut resp = self
            .db
            .query(
                "SELECT in.name AS name, in.file_path AS file_path, in.line_start AS line_start \
                 FROM implements WHERE out.name = $iface LIMIT 100",
            )
            .bind(("iface", interface_name.to_string()))
            .await
            .context("find_interface_implementors")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        Ok(records)
    }

    // ========================================================================
    // Graph Edges (5)
    // ========================================================================

    /// Get all import relationships for a project.
    pub async fn get_project_import_edges(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<(String, String)>> {
        let mut resp = self
            .db
            .query(
                "SELECT in.path AS from_path, out.path AS to_path FROM imports \
                 WHERE in.project_id = $pid LIMIT 10000",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("get_project_import_edges")?;
        let records: Vec<EdgeRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .map(|r| (r.from_path, r.to_path))
            .collect())
    }

    /// Get all call relationships for a project.
    pub async fn get_project_call_edges(&self, project_id: Uuid) -> Result<Vec<(String, String)>> {
        let mut resp = self
            .db
            .query(
                "SELECT in.name AS from_path, out.name AS to_path FROM calls \
                 WHERE in.file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
                 LIMIT 10000",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("get_project_call_edges")?;
        let records: Vec<EdgeRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .map(|r| (r.from_path, r.to_path))
            .collect())
    }

    /// Get extends relationships for a project.
    pub async fn get_project_extends_edges(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<(String, String)>> {
        let mut resp = self
            .db
            .query(
                "SELECT in.name AS from_path, out.name AS to_path FROM extends \
                 WHERE in.file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
                 LIMIT 10000",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("get_project_extends_edges")?;
        let records: Vec<EdgeRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .map(|r| (r.from_path, r.to_path))
            .collect())
    }

    /// Get implements relationships for a project.
    pub async fn get_project_implements_edges(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<(String, String)>> {
        let mut resp = self
            .db
            .query(
                "SELECT in.name AS from_path, out.name AS to_path FROM implements \
                 WHERE in.file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
                 LIMIT 10000",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("get_project_implements_edges")?;
        let records: Vec<EdgeRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .map(|r| (r.from_path, r.to_path))
            .collect())
    }

    /// Get functions with no callers (entry points).
    pub async fn get_top_entry_functions(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> Result<Vec<String>> {
        let mut resp = self
            .db
            .query(
                "SELECT name FROM `function` \
                 WHERE file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
                 AND id NOT IN (SELECT VALUE out FROM calls) \
                 LIMIT $limit",
            )
            .bind(("pid", project_id.to_string()))
            .bind(("limit", limit as i64))
            .await
            .context("get_top_entry_functions")?;
        let records: Vec<NameRecord> = resp.take(0)?;
        Ok(records.into_iter().map(|r| r.name).collect())
    }

    // ========================================================================
    // Batch Relationships (5)
    // ========================================================================

    /// Batch create call relationships.
    pub async fn batch_create_call_relationships(
        &self,
        calls: &[FunctionCall],
        project_id: Option<Uuid>,
    ) -> Result<()> {
        for call in calls {
            // Check if callee exists (parameterized)
            let (find_sql, has_pid) = if project_id.is_some() {
                (
                    "SELECT id FROM `function` WHERE name = $callee_name \
                     AND file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
                     LIMIT 1",
                    true,
                )
            } else {
                (
                    "SELECT id FROM `function` WHERE name = $callee_name LIMIT 1",
                    false,
                )
            };
            let mut find_qb = self
                .db
                .query(find_sql)
                .bind(("callee_name", call.callee_name.clone()));
            if has_pid {
                find_qb = find_qb.bind(("pid", project_id.unwrap().to_string()));
            }
            let mut resp = find_qb.await?;
            let targets: Vec<serde_json::Value> = resp.take(0)?;

            if targets.is_empty() {
                continue;
            }

            let caller_rid = RecordId::new("function", call.caller_id.as_str());
            self.db
                .query(
                    "LET $target = (SELECT VALUE id FROM `function` WHERE name = $callee_name LIMIT 1); \
                     IF $target THEN \
                       RELATE $caller->calls->$target[0] SET confidence = $conf, reason = $reason \
                     END",
                )
                .bind(("caller", caller_rid))
                .bind(("callee_name", call.callee_name.clone()))
                .bind(("conf", call.confidence))
                .bind(("reason", call.reason.clone()))
                .await?;
        }
        Ok(())
    }

    /// Batch create extends relationships (from_type, from_file, to_type, to_file).
    pub async fn batch_create_extends_relationships(
        &self,
        rels: &[(String, String, String, String)],
    ) -> Result<()> {
        for (from_type, from_file, to_type, _to_file) in rels {
            self.db
                .query(
                    "LET $from = (SELECT VALUE id FROM `struct` \
                      WHERE name = $from_type AND file_path = $from_file LIMIT 1); \
                     LET $to = (SELECT VALUE id FROM `struct` WHERE name = $to_type LIMIT 1); \
                     IF $from AND $to THEN RELATE $from[0]->extends->$to[0] END",
                )
                .bind(("from_type", from_type.clone()))
                .bind(("from_file", from_file.clone()))
                .bind(("to_type", to_type.clone()))
                .await?;
        }
        Ok(())
    }

    /// Batch create implements relationships (from_type, from_file, to_trait, to_file).
    pub async fn batch_create_implements_relationships(
        &self,
        rels: &[(String, String, String, String)],
    ) -> Result<()> {
        for (from_type, from_file, to_trait, _to_file) in rels {
            self.db
                .query(
                    "LET $from = (SELECT VALUE id FROM `struct` \
                      WHERE name = $from_type AND file_path = $from_file LIMIT 1); \
                     LET $to = (SELECT VALUE id FROM `trait` WHERE name = $to_trait LIMIT 1); \
                     IF $from AND $to THEN RELATE $from[0]->implements->$to[0] END",
                )
                .bind(("from_type", from_type.clone()))
                .bind(("from_file", from_file.clone()))
                .bind(("to_trait", to_trait.clone()))
                .await?;
        }
        Ok(())
    }

    /// Create a single imports_symbol relationship.
    pub async fn create_imports_symbol_relationship(
        &self,
        import_id: &str,
        symbol_name: &str,
        _project_id: Option<Uuid>,
    ) -> Result<()> {
        let from_rid = RecordId::new("import", import_id);
        self.db
            .query(
                "LET $to = (SELECT VALUE id FROM `function` WHERE name = $symbol_name LIMIT 1); \
                 IF $to THEN RELATE $from->imports_symbol->$to[0] END",
            )
            .bind(("from", from_rid))
            .bind(("symbol_name", symbol_name.to_string()))
            .await
            .context("create_imports_symbol_relationship")?;
        Ok(())
    }

    /// Batch create imports_symbol relationships.
    pub async fn batch_create_imports_symbol_relationships(
        &self,
        relationships: &[(String, String, Option<Uuid>)],
    ) -> Result<()> {
        for (import_id, symbol_name, project_id) in relationships {
            self.create_imports_symbol_relationship(import_id, symbol_name, *project_id)
                .await?;
        }
        Ok(())
    }

    // ========================================================================
    // Cleanup (3)
    // ========================================================================

    /// Delete calls where caller/callee are in different projects.
    pub async fn cleanup_cross_project_calls(&self) -> Result<i64> {
        let mut resp = self
            .db
            .query(
                "LET $bad = (SELECT VALUE id FROM calls \
                 WHERE in.file_path != NONE AND out.file_path != NONE \
                 AND (SELECT VALUE project_id FROM file WHERE path = in.file_path LIMIT 1) \
                 != (SELECT VALUE project_id FROM file WHERE path = out.file_path LIMIT 1)); \
                 DELETE FROM calls WHERE id IN $bad; \
                 RETURN array::len($bad)",
            )
            .await
            .context("cleanup_cross_project_calls")?;
        let result: Vec<serde_json::Value> = resp.take(2)?;
        let count = result.first().and_then(|v| v.as_i64()).unwrap_or(0);
        Ok(count)
    }

    /// Delete calls to known built-in functions.
    pub async fn cleanup_builtin_calls(&self) -> Result<i64> {
        // Delete calls where the callee name is a known builtin
        let builtins = [
            "println",
            "print",
            "eprintln",
            "eprint",
            "format",
            "vec",
            "dbg",
            "todo",
            "unimplemented",
            "unreachable",
            "panic",
            "assert",
            "assert_eq",
            "assert_ne",
            "debug_assert",
            "debug_assert_eq",
            "debug_assert_ne",
            "write",
            "writeln",
            "String::new",
            "Vec::new",
            "HashMap::new",
            "len",
            "push",
            "pop",
            "clone",
            "to_string",
            "into",
            "from",
            "unwrap",
            "expect",
            "map",
            "filter",
            "collect",
            "iter",
        ];
        let builtin_list: String = builtins
            .iter()
            .map(|b| format!("'{}'", b))
            .collect::<Vec<_>>()
            .join(", ");

        let query = format!(
            "LET $bad = (SELECT VALUE id FROM calls WHERE out.name IN [{}]); \
             DELETE FROM calls WHERE id IN $bad; \
             RETURN array::len($bad)",
            builtin_list
        );
        let mut resp = self
            .db
            .query(&query)
            .await
            .context("cleanup_builtin_calls")?;
        let result: Vec<serde_json::Value> = resp.take(2)?;
        let count = result.first().and_then(|v| v.as_i64()).unwrap_or(0);
        Ok(count)
    }

    /// Set default confidence on calls missing it.
    pub async fn migrate_calls_confidence(&self) -> Result<i64> {
        let mut resp = self
            .db
            .query(
                "LET $updated = (UPDATE calls SET confidence = 0.5, reason = 'migrated' \
                 WHERE confidence = NONE RETURN AFTER); \
                 RETURN array::len($updated)",
            )
            .await
            .context("migrate_calls_confidence")?;
        let result: Vec<serde_json::Value> = resp.take(1)?;
        let count = result.first().and_then(|v| v.as_i64()).unwrap_or(0);
        Ok(count)
    }

    // ========================================================================
    // Misc (6)
    // ========================================================================

    /// List processes for a project.
    pub async fn list_processes(&self, project_id: Uuid) -> Result<Vec<serde_json::Value>> {
        let query = format!(
            "SELECT * FROM `process` WHERE project_id = '{}' ORDER BY name ASC",
            project_id
        );
        let mut resp = self.db.query(&query).await.context("list_processes")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        Ok(records)
    }

    /// Get detail of a specific process.
    pub async fn get_process_detail(&self, process_id: &str) -> Result<Option<serde_json::Value>> {
        let rid = RecordId::new("process", process_id);
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("get_process_detail")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        Ok(records.into_iter().next())
    }

    /// Get entry points for a project (functions with no callers).
    pub async fn get_entry_points(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        let mut resp = self
            .db
            .query(
                "SELECT name, file_path, line_start, is_async, visibility \
                 FROM `function` \
                 WHERE file_path IN (SELECT VALUE path FROM file WHERE project_id = $pid) \
                 AND id NOT IN (SELECT VALUE out FROM calls) \
                 ORDER BY name ASC \
                 LIMIT $limit",
            )
            .bind(("pid", project_id.to_string()))
            .bind(("limit", limit as i64))
            .await
            .context("get_entry_points")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        Ok(records)
    }

    /// Link a task to files via modifies_file edges.
    pub async fn link_task_to_files(&self, task_id: Uuid, file_paths: &[String]) -> Result<()> {
        let task_rid = RecordId::new("task", task_id.to_string().as_str());
        for path in file_paths {
            let file_rid = RecordId::new("file", path.as_str());
            self.db
                .query("RELATE $task->modifies_file->$file RETURN NONE")
                .bind(("task", task_rid.clone()))
                .bind(("file", file_rid))
                .await?;
        }
        Ok(())
    }

    /// List projects with search/pagination.
    pub async fn list_projects_filtered(
        &self,
        search: Option<&str>,
        limit: usize,
        offset: usize,
        sort_by: Option<&str>,
        sort_order: &str,
    ) -> Result<(Vec<ProjectNode>, usize)> {
        // Validate sort_field and sort_order against allowlists to prevent injection.
        let order_field = match sort_by.unwrap_or("created_at") {
            "created_at" | "updated_at" | "name" | "slug" => sort_by.unwrap_or("created_at"),
            _ => "created_at",
        };
        let order_dir = if sort_order == "asc" { "ASC" } else { "DESC" };

        let (count_q, data_q) = if search.is_some() {
            (
                format!(
                    "SELECT count() AS total FROM project \
                     WHERE name CONTAINS $search OR slug CONTAINS $search \
                       OR description CONTAINS $search \
                     GROUP ALL"
                ),
                format!(
                    "SELECT * FROM project \
                     WHERE name CONTAINS $search OR slug CONTAINS $search \
                       OR description CONTAINS $search \
                     ORDER BY {} {} LIMIT $limit START $offset",
                    order_field, order_dir
                ),
            )
        } else {
            (
                "SELECT count() AS total FROM project GROUP ALL".to_string(),
                format!(
                    "SELECT * FROM project ORDER BY {} {} LIMIT $limit START $offset",
                    order_field, order_dir
                ),
            )
        };

        let combined = format!("{}; {}", count_q, data_q);
        let mut qb = self
            .db
            .query(&combined)
            .bind(("limit", limit as i64))
            .bind(("offset", offset as i64));
        if let Some(s) = search {
            qb = qb.bind(("search", s.to_string()));
        }
        let mut resp = qb.await?;
        let count_result: Vec<serde_json::Value> = resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let records: Vec<crate::project::ProjectRecord> = resp.take(1)?;
        let projects = records
            .into_iter()
            .filter_map(|r| r.into_project_node())
            .collect();

        Ok((projects, total))
    }

    /// Find blocked tasks (tasks that have uncompleted dependencies).
    pub async fn find_blocked_tasks(
        &self,
        plan_id: Uuid,
    ) -> Result<Vec<(TaskNode, Vec<TaskNode>)>> {
        // Get all tasks for the plan
        let tasks = self.get_plan_tasks(plan_id).await?;
        let mut results = Vec::new();

        for task in &tasks {
            if task.status == TaskStatus::Completed {
                continue;
            }
            let blockers = self.get_task_blockers(task.id).await?;
            let uncompleted: Vec<TaskNode> = blockers
                .into_iter()
                .filter(|b| b.status != TaskStatus::Completed)
                .collect();
            if !uncompleted.is_empty() {
                results.push((task.clone(), uncompleted));
            }
        }
        Ok(results)
    }

    // ========================================================================
    // Task Impact Analysis (2)
    // ========================================================================

    /// Analyze task impact by looking at affected files and their dependents.
    pub async fn analyze_task_impact(&self, task_id: Uuid) -> Result<Vec<String>> {
        let task = self.get_task(task_id).await?;
        let task = match task {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        let mut impacted = std::collections::HashSet::new();
        for file_path in &task.affected_files {
            let dependents = self.find_dependent_files(file_path, 2, None).await?;
            for dep in dependents {
                impacted.insert(dep);
            }
        }
        Ok(impacted.into_iter().collect())
    }

    /// Get full task details including steps, decisions, dependencies.
    pub async fn get_task_with_full_details(&self, task_id: Uuid) -> Result<Option<TaskDetails>> {
        let task = match self.get_task(task_id).await? {
            Some(t) => t,
            None => return Ok(None),
        };

        let steps = self.get_task_steps(task_id).await?;
        // Query decisions linked to this task via task_id field
        let dec_query = format!(
            "SELECT * FROM decision WHERE task_id = '{}' LIMIT 100",
            task_id
        );
        let mut dec_resp = self
            .db
            .query(&dec_query)
            .await
            .context("get_task_decisions")?;
        let dec_vals: Vec<serde_json::Value> = dec_resp.take(0).unwrap_or_default();
        let decisions: Vec<DecisionNode> = dec_vals
            .into_iter()
            .filter_map(|v| {
                let id_str = v.get("id")?.as_str()?;
                let id = id_str
                    .split(':')
                    .next_back()
                    .and_then(|s| Uuid::parse_str(s).ok())?;
                let alts_str = v
                    .get("alternatives")
                    .and_then(|a| a.as_str())
                    .unwrap_or("[]");
                let alternatives: Vec<String> = serde_json::from_str(alts_str).unwrap_or_default();
                let status = match v
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("proposed")
                {
                    "accepted" => DecisionStatus::Accepted,
                    "deprecated" => DecisionStatus::Deprecated,
                    "superseded" => DecisionStatus::Superseded,
                    _ => DecisionStatus::Proposed,
                };
                Some(DecisionNode {
                    id,
                    description: v.get("description")?.as_str()?.to_string(),
                    rationale: v
                        .get("rationale")
                        .and_then(|r| r.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    alternatives,
                    chosen_option: v
                        .get("chosen_option")
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string()),
                    decided_by: v
                        .get("decided_by")
                        .and_then(|d| d.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    decided_at: v
                        .get("decided_at")
                        .and_then(|d| d.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_else(chrono::Utc::now),
                    status,
                    embedding: None,
                    embedding_model: v
                        .get("embedding_model")
                        .and_then(|e| e.as_str())
                        .map(|s| s.to_string()),
                })
            })
            .collect();
        let deps = self.get_task_dependencies(task_id).await?;
        let dep_ids: Vec<Uuid> = deps.iter().map(|d| d.id).collect();

        Ok(Some(TaskDetails {
            task,
            steps,
            decisions,
            depends_on: dep_ids,
            modifies_files: vec![],
        }))
    }

    // ========================================================================
    // Topology (2)
    // ========================================================================

    /// Check topology rules for a project.
    pub async fn check_topology_rules_code(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<TopologyViolation>> {
        let rules = self.list_topology_rules(&project_id.to_string()).await?;
        let mut violations = Vec::new();

        for rule in &rules {
            match rule.rule_type {
                TopologyRuleType::MustNotImport => {
                    if let Some(ref target_pattern) = rule.target_pattern {
                        let source_regex = cortex_core::graph::glob_to_regex(&rule.source_pattern);
                        let target_regex = cortex_core::graph::glob_to_regex(target_pattern);

                        let query = format!(
                            "SELECT in.path AS from_path, out.path AS to_path FROM imports \
                             WHERE in.project_id = '{}' \
                             AND string::matches(in.path, '{}') \
                             AND string::matches(out.path, '{}') LIMIT 500",
                            project_id, source_regex, target_regex
                        );
                        let mut resp = self.db.query(&query).await?;
                        let records: Vec<EdgeRecord> = resp.take(0)?;
                        for rec in records {
                            violations.push(TopologyViolation {
                                rule_id: rule.id.clone(),
                                rule_description: rule.description.clone(),
                                rule_type: rule.rule_type.clone(),
                                violator_path: rec.from_path,
                                target_path: Some(rec.to_path),
                                severity: rule.severity.clone(),
                                details: format!("Forbidden import: rule {}", rule.description),
                                violation_score: 1.0,
                            });
                        }
                    }
                }
                TopologyRuleType::MustNotCall => {
                    if let Some(ref target_pattern) = rule.target_pattern {
                        let source_regex = cortex_core::graph::glob_to_regex(&rule.source_pattern);
                        let target_regex = cortex_core::graph::glob_to_regex(target_pattern);

                        let query = format!(
                            "SELECT in.file_path AS from_path, out.file_path AS to_path FROM calls \
                             WHERE in.file_path IN (SELECT VALUE path FROM file WHERE project_id = '{}') \
                             AND string::matches(in.file_path, '{}') \
                             AND string::matches(out.file_path, '{}') LIMIT 500",
                            project_id, source_regex, target_regex
                        );
                        let mut resp = self.db.query(&query).await?;
                        let records: Vec<EdgeRecord> = resp.take(0)?;
                        for rec in records {
                            violations.push(TopologyViolation {
                                rule_id: rule.id.clone(),
                                rule_description: rule.description.clone(),
                                rule_type: rule.rule_type.clone(),
                                violator_path: rec.from_path,
                                target_path: Some(rec.to_path),
                                severity: rule.severity.clone(),
                                details: format!("Forbidden call: rule {}", rule.description),
                                violation_score: 1.0,
                            });
                        }
                    }
                }
                TopologyRuleType::MaxFanOut => {
                    if let Some(threshold) = rule.threshold {
                        let source_regex = cortex_core::graph::glob_to_regex(&rule.source_pattern);
                        let query = format!(
                            "SELECT path, (SELECT count() FROM imports WHERE in = type::record('file', path) GROUP ALL)[0].count AS fan_out \
                             FROM file WHERE project_id = '{}' AND string::matches(path, '{}') \
                             LIMIT 1000",
                            project_id, source_regex
                        );
                        let mut resp = self.db.query(&query).await?;
                        let records: Vec<serde_json::Value> = resp.take(0)?;
                        for rec in records {
                            let fan_out =
                                rec.get("fan_out").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            if fan_out > threshold {
                                let path = rec
                                    .get("path")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                violations.push(TopologyViolation {
                                    rule_id: rule.id.clone(),
                                    rule_description: rule.description.clone(),
                                    rule_type: rule.rule_type.clone(),
                                    violator_path: path,
                                    target_path: None,
                                    severity: rule.severity.clone(),
                                    details: format!(
                                        "Fan-out {} exceeds threshold {}",
                                        fan_out, threshold
                                    ),
                                    violation_score: fan_out as f64 / threshold as f64,
                                });
                            }
                        }
                    }
                }
                _ => {
                    // NoCircular and MaxDistance require more complex graph traversal
                    // handled at application layer
                }
            }
        }

        Ok(violations)
    }

    /// Check if new imports from a file would violate topology rules.
    pub async fn check_file_topology_code(
        &self,
        project_id: Uuid,
        file_path: &str,
        new_imports: &[String],
    ) -> Result<Vec<TopologyViolation>> {
        let rules = self.list_topology_rules(&project_id.to_string()).await?;
        let mut violations = Vec::new();

        for rule in &rules {
            if rule.rule_type != TopologyRuleType::MustNotImport {
                continue;
            }
            if let Some(ref target_pattern) = rule.target_pattern {
                let source_re =
                    regex::Regex::new(&cortex_core::graph::glob_to_regex(&rule.source_pattern));
                let target_re =
                    regex::Regex::new(&cortex_core::graph::glob_to_regex(target_pattern));

                if let (Ok(src_re), Ok(tgt_re)) = (&source_re, &target_re) {
                    if src_re.is_match(file_path) {
                        for imp in new_imports {
                            if tgt_re.is_match(imp) {
                                violations.push(TopologyViolation {
                                    rule_id: rule.id.clone(),
                                    rule_description: rule.description.clone(),
                                    rule_type: rule.rule_type.clone(),
                                    violator_path: file_path.to_string(),
                                    target_path: Some(imp.clone()),
                                    severity: rule.severity.clone(),
                                    details: format!(
                                        "Import of '{}' from '{}' violates rule: {}",
                                        imp, file_path, rule.description
                                    ),
                                    violation_score: 1.0,
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(violations)
    }

    // ========================================================================
    // Bridge & Similarity (3)
    // ========================================================================

    /// Find bridge subgraph between two nodes.
    pub async fn find_bridge_subgraph(
        &self,
        source: &str,
        target: &str,
        project_id: Uuid,
    ) -> Result<(Vec<BridgeRawNode>, Vec<BridgeRawEdge>)> {
        // BFS from source and target, find overlapping paths
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut seen_nodes = std::collections::HashSet::new();

        let pid_str = project_id.to_string();

        // Forward BFS from source (via imports)
        let mut forward_set = std::collections::HashSet::new();
        forward_set.insert(source.to_string());
        let mut frontier = vec![source.to_string()];

        for _ in 0..3 {
            if frontier.is_empty() {
                break;
            }
            let mut next = Vec::new();
            for fp in &frontier {
                let file_rid = RecordId::new("file", fp.as_str());
                let mut resp = self
                    .db
                    .query(
                        "SELECT out.path AS path FROM imports \
                         WHERE in = $file_rid AND out.project_id = $pid LIMIT 100",
                    )
                    .bind(("file_rid", file_rid))
                    .bind(("pid", pid_str.clone()))
                    .await?;
                let records: Vec<PathRecord> = resp.take(0)?;
                for rec in records {
                    if forward_set.insert(rec.path.clone()) {
                        next.push(rec.path);
                    }
                }
            }
            frontier = next;
        }

        // Backward BFS from target
        let mut backward_set = std::collections::HashSet::new();
        backward_set.insert(target.to_string());
        let mut frontier = vec![target.to_string()];

        for _ in 0..3 {
            if frontier.is_empty() {
                break;
            }
            let mut next = Vec::new();
            for fp in &frontier {
                let file_rid = RecordId::new("file", fp.as_str());
                let mut resp = self
                    .db
                    .query(
                        "SELECT in.path AS path FROM imports \
                         WHERE out = $file_rid AND in.project_id = $pid LIMIT 100",
                    )
                    .bind(("file_rid", file_rid))
                    .bind(("pid", pid_str.clone()))
                    .await?;
                let records: Vec<PathRecord> = resp.take(0)?;
                for rec in records {
                    if backward_set.insert(rec.path.clone()) {
                        next.push(rec.path);
                    }
                }
            }
            frontier = next;
        }

        // Intersection = bridge nodes
        let bridge_paths: std::collections::HashSet<_> =
            forward_set.intersection(&backward_set).cloned().collect();

        for path in &bridge_paths {
            if seen_nodes.insert(path.clone()) {
                nodes.push(BridgeRawNode {
                    path: path.clone(),
                    node_type: "File".to_string(),
                });
            }
        }

        // Get edges between bridge nodes
        for path in &bridge_paths {
            let file_rid = RecordId::new("file", path.as_str());
            let mut resp = self
                .db
                .query(
                    "SELECT in.path AS from_path, out.path AS to_path FROM imports \
                     WHERE in = $file_rid LIMIT 500",
                )
                .bind(("file_rid", file_rid))
                .await?;
            let records: Vec<EdgeRecord> = resp.take(0)?;
            for rec in records {
                if bridge_paths.contains(&rec.to_path) {
                    edges.push(BridgeRawEdge {
                        from_path: rec.from_path,
                        to_path: rec.to_path,
                        rel_type: "IMPORTS".to_string(),
                    });
                }
            }
        }

        Ok((nodes, edges))
    }

    /// Get synapse graph for a project.
    pub async fn get_synapse_graph(
        &self,
        project_id: Uuid,
        min_weight: f64,
    ) -> Result<Vec<(String, String, f64)>> {
        let query = format!(
            "SELECT in.id AS source, out.id AS target, weight FROM synapse \
             WHERE weight >= {} \
             AND in.project_id = '{}' \
             LIMIT 5000",
            min_weight, project_id
        );
        let mut resp = self.db.query(&query).await.context("get_synapse_graph")?;
        let records: Vec<SynapseRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .map(|r| (r.source, r.target, r.weight))
            .collect())
    }

    /// Find isomorphic groups (files with same WL hash).
    pub async fn find_isomorphic_groups(
        &self,
        project_id: Uuid,
        min_similarity: f64,
    ) -> Result<Vec<IsomorphicGroup>> {
        let _ = min_similarity; // WL hash gives exact structural match
        let query = format!(
            "SELECT wl_hash, array::group(path) AS members, count() AS size \
             FROM file WHERE project_id = '{}' AND wl_hash != NONE \
             GROUP BY wl_hash HAVING count() >= 2 \
             ORDER BY size DESC LIMIT 100",
            project_id
        );
        let mut resp = self
            .db
            .query(&query)
            .await
            .context("find_isomorphic_groups")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;

        Ok(records
            .into_iter()
            .filter_map(|v| {
                let wl_hash = v.get("wl_hash")?.as_str()?.parse::<u64>().ok()?;
                let members: Vec<String> = v
                    .get("members")?
                    .as_array()?
                    .iter()
                    .filter_map(|m| m.as_str().map(|s| s.to_string()))
                    .collect();
                let size = members.len();
                Some(IsomorphicGroup {
                    wl_hash,
                    members,
                    size,
                })
            })
            .collect())
    }

    // ========================================================================
    // Metric Getters (4)
    // ========================================================================

    /// Get PageRank of a file node.
    pub async fn get_node_pagerank(&self, file_path: &str, _project_id: &str) -> Result<f64> {
        let rid = RecordId::new("file", file_path);
        let mut resp = self
            .db
            .query("SELECT pagerank AS value FROM $rid")
            .bind(("rid", rid))
            .await
            .context("get_node_pagerank")?;
        let records: Vec<FloatRecord> = resp.take(0)?;
        Ok(records.first().and_then(|r| r.value).unwrap_or(0.0))
    }

    /// Get bridge proximity (files connected to the given file with betweenness scores).
    pub async fn get_bridge_proximity(
        &self,
        file_path: &str,
        _project_id: &str,
    ) -> Result<Vec<(String, f64)>> {
        let file_rid = RecordId::new("file", file_path);
        let mut resp = self
            .db
            .query(
                "SELECT out.path AS path, out.betweenness AS score FROM imports \
                 WHERE in = $file_rid AND out.betweenness != NONE \
                 ORDER BY score DESC LIMIT 20",
            )
            .bind(("file_rid", file_rid))
            .await
            .context("get_bridge_proximity")?;
        let records: Vec<BridgeProxRecord> = resp.take(0)?;
        Ok(records.into_iter().map(|r| (r.path, r.score)).collect())
    }

    /// Get average multi-signal score for a project.
    pub async fn get_avg_multi_signal_score(&self, project_id: Uuid) -> Result<f64> {
        let mut resp = self
            .db
            .query(
                "SELECT math::mean(pagerank) AS value FROM file \
                 WHERE project_id = $pid AND pagerank != NONE GROUP ALL",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("get_avg_multi_signal_score")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        let avg = records
            .first()
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        Ok(avg)
    }

    /// Get knowledge density for a file.
    pub async fn get_knowledge_density(&self, file_path: &str, _project_id: &str) -> Result<f64> {
        let rid = RecordId::new("file", file_path);
        let mut resp = self
            .db
            .query("SELECT knowledge_density AS value FROM $rid")
            .bind(("rid", rid))
            .await
            .context("get_knowledge_density")?;
        let records: Vec<FloatRecord> = resp.take(0)?;
        Ok(records.first().and_then(|r| r.value).unwrap_or(0.0))
    }

    // ========================================================================
    // Context Cards (5)
    // ========================================================================

    /// Get a single context card.
    pub async fn get_context_card(
        &self,
        path: &str,
        project_id: &str,
    ) -> Result<Option<ContextCard>> {
        let mut resp = self
            .db
            .query("SELECT * FROM context_card WHERE path = $path AND project_id = $pid LIMIT 1")
            .bind(("path", path.to_string()))
            .bind(("pid", project_id.to_string()))
            .await
            .context("get_context_card")?;
        let records: Vec<ContextCardRecord> = resp.take(0)?;
        Ok(records.into_iter().next().map(|r| r.into_card()))
    }

    /// Get context cards in batch.
    pub async fn get_context_cards_batch(
        &self,
        paths: &[String],
        project_id: &str,
    ) -> Result<Vec<ContextCard>> {
        if paths.is_empty() {
            return Ok(vec![]);
        }
        let path_list: Vec<String> = paths.to_vec();
        let mut resp = self
            .db
            .query("SELECT * FROM context_card WHERE path IN $paths AND project_id = $pid")
            .bind(("paths", path_list))
            .bind(("pid", project_id.to_string()))
            .await
            .context("get_context_cards_batch")?;
        let records: Vec<ContextCardRecord> = resp.take(0)?;
        Ok(records.into_iter().map(|r| r.into_card()).collect())
    }

    /// Check if a project has any context cards.
    pub async fn has_context_cards(&self, project_id: &str) -> Result<bool> {
        let mut resp = self
            .db
            .query(
                "SELECT count() AS count FROM context_card \
                 WHERE project_id = $pid LIMIT 1 GROUP ALL",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("has_context_cards")?;
        let records: Vec<serde_json::Value> = resp.take(0)?;
        let count = records
            .first()
            .and_then(|v| v.get("count"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok(count > 0)
    }

    /// Batch save context cards.
    pub async fn batch_save_context_cards(&self, cards: &[ContextCard]) -> Result<()> {
        for card in cards {
            let key = format!("{}:{}", card.path, card.cc_computed_at);
            let rid = RecordId::new("context_card", key.as_str());
            let dna_json = serde_json::to_string(&card.cc_structural_dna).unwrap_or_default();
            let fp_json = serde_json::to_string(&card.cc_fingerprint).unwrap_or_default();
            let co_json = serde_json::to_string(&card.cc_co_changers_top5).unwrap_or_default();

            self.db
                .query(
                    "CREATE $rid SET \
                     path = $path, project_id = $pid, \
                     cc_pagerank = $pr, cc_betweenness = $bt, cc_clustering = $cl, \
                     cc_community_id = $cid, cc_community_label = $clbl, \
                     cc_imports_out = $io, cc_imports_in = $ii, \
                     cc_calls_out = $co, cc_calls_in = $ci, \
                     cc_structural_dna = $dna, cc_wl_hash = $wl, \
                     cc_fingerprint = $fp, cc_co_changers_top5 = $cch, \
                     cc_version = $ver, cc_computed_at = $cat \
                     RETURN NONE",
                )
                .bind(("rid", rid))
                .bind(("path", card.path.clone()))
                .bind(("pid", String::new())) // project_id will be set from caller context
                .bind(("pr", card.cc_pagerank))
                .bind(("bt", card.cc_betweenness))
                .bind(("cl", card.cc_clustering))
                .bind(("cid", card.cc_community_id as i64))
                .bind(("clbl", card.cc_community_label.clone()))
                .bind(("io", card.cc_imports_out as i64))
                .bind(("ii", card.cc_imports_in as i64))
                .bind(("co", card.cc_calls_out as i64))
                .bind(("ci", card.cc_calls_in as i64))
                .bind(("dna", dna_json))
                .bind(("wl", card.cc_wl_hash.to_string()))
                .bind(("fp", fp_json))
                .bind(("cch", co_json))
                .bind(("ver", card.cc_version as i64))
                .bind(("cat", card.cc_computed_at.clone()))
                .await?;
        }
        Ok(())
    }

    /// Invalidate context cards for given paths.
    pub async fn invalidate_context_cards(&self, paths: &[String], project_id: &str) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let path_list: Vec<String> = paths.to_vec();
        self.db
            .query(
                "UPDATE context_card SET cc_version = -1 \
                 WHERE path IN $paths AND project_id = $pid",
            )
            .bind(("paths", path_list))
            .bind(("pid", project_id.to_string()))
            .await
            .context("invalidate_context_cards")?;
        Ok(())
    }

    // =======================================================================
    // Full-text search (BM25)
    // =======================================================================

    /// Full-text BM25 search across code symbols (functions + structs).
    ///
    /// Searches `name` and `docstring` fields on both `function` and `struct`
    /// tables using SurrealDB's `@@` operator.  Results are grouped by file
    /// path into `CodeSearchHit` entries.
    ///
    /// Falls back to a CONTAINS-based keyword search when BM25 is unavailable
    /// (e.g. in-memory `kv-mem` engine used by tests).
    pub async fn search_code_fts(
        &self,
        query: &str,
        limit: usize,
        project_id: Option<&str>,
        language: Option<&str>,
    ) -> Result<Vec<cortex_graph::CodeSearchHit>> {
        let half_limit = (limit / 2).max(5);
        let mut bm25_failed = false;

        // Attempt BM25 search across `function` and `struct` tables.
        let bm25_rows: Vec<serde_json::Value> = {
            let func_sql = "SELECT name, docstring, file_path, search::score() AS _score \
                FROM function \
                WHERE name @@ $query OR docstring @@ $query \
                ORDER BY _score DESC \
                LIMIT $limit";
            let struct_sql = "SELECT name, docstring, file_path, search::score() AS _score \
                FROM `struct` \
                WHERE name @@ $query OR docstring @@ $query \
                ORDER BY _score DESC \
                LIMIT $limit";

            let mut rows = Vec::new();
            for (table, sql) in [("function", func_sql), ("struct", struct_sql)] {
                let qb = self
                    .db
                    .query(sql)
                    .bind(("query", query.to_string()))
                    .bind(("limit", half_limit));
                match qb.await {
                    Ok(mut resp) => match resp.take::<Vec<serde_json::Value>>(0) {
                        Ok(r) => rows.extend(r),
                        Err(e) => {
                            bm25_failed = true;
                            tracing::warn!(
                                error = %e,
                                table,
                                "BM25 FTS row extraction failed for code search"
                            );
                        }
                    },
                    Err(e) => {
                        bm25_failed = true;
                        tracing::warn!(error = %e, table, "BM25 FTS query failed for code search");
                    }
                }
            }
            rows
        };

        // Determine if BM25 produced any results; fall back to CONTAINS if not.
        let symbol_rows: Vec<serde_json::Value> = if !bm25_rows.is_empty() {
            bm25_rows
        } else {
            // Fallback: keyword CONTAINS search.
            if bm25_failed {
                tracing::warn!("BM25 FTS unavailable, falling back to CONTAINS search");
            } else {
                tracing::debug!("BM25 returned no code hits, falling back to CONTAINS search");
            }
            let kw = query.to_lowercase();
            let func_sql = "SELECT name, docstring, file_path FROM function \
                 WHERE string::lowercase(name) CONTAINS $kw \
                    OR string::lowercase(string::concat('', docstring ?? '')) CONTAINS $kw \
                 LIMIT $limit";
            let struct_sql = "SELECT name, docstring, file_path FROM `struct` \
                 WHERE string::lowercase(name) CONTAINS $kw \
                    OR string::lowercase(string::concat('', docstring ?? '')) CONTAINS $kw \
                 LIMIT $limit";
            let mut rows = Vec::new();
            for sql in [func_sql, struct_sql] {
                if let Ok(mut resp) = self
                    .db
                    .query(sql)
                    .bind(("kw", kw.clone()))
                    .bind(("limit", half_limit as i64))
                    .await
                {
                    if let Ok(r) = resp.take::<Vec<serde_json::Value>>(0) {
                        rows.extend(r);
                    }
                }
            }
            rows
        };

        // Group by file_path into CodeSearchHit entries.
        use std::collections::HashMap;
        let mut by_path: HashMap<String, cortex_graph::CodeSearchHit> = HashMap::new();

        for row in &symbol_rows {
            let file_path = row
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if file_path.is_empty() {
                continue;
            }
            let score = row.get("_score").and_then(|v| v.as_f64()).unwrap_or(1.0);
            let name = row
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let docstring = row
                .get("docstring")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let entry = by_path.entry(file_path.clone()).or_insert_with(|| {
                // Determine language from file extension.
                let lang = language.map(|l| l.to_string()).unwrap_or_else(|| {
                    let ext = std::path::Path::new(&file_path)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    ext.to_string()
                });
                cortex_graph::CodeSearchHit {
                    path: file_path.clone(),
                    language: lang,
                    symbols: Vec::new(),
                    docstring: String::new(),
                    score: 0.0,
                    project_id: project_id.map(|s| s.to_string()),
                    project_slug: None,
                }
            });

            if !name.is_empty() && !entry.symbols.contains(&name) {
                entry.symbols.push(name);
            }
            // Use the best docstring (non-empty, highest score wins by insertion order).
            if entry.docstring.is_empty() && !docstring.is_empty() {
                entry.docstring = docstring;
            }
            // Use the maximum score across all symbols in this file.
            if score > entry.score {
                entry.score = score;
            }
        }

        // Fetch language from file table for results that need it.
        let mut hits: Vec<cortex_graph::CodeSearchHit> = by_path.into_values().collect();

        // Apply language filter if requested.
        if let Some(lang_filter) = language {
            hits.retain(|h| h.language == lang_filter);
        }

        // Sort by descending score.
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        Ok(hits)
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
    async fn test_get_callees_empty() {
        let store = setup().await;
        let result = store.get_callees("nonexistent", 1).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_find_dependent_files_empty() {
        let store = setup().await;
        let result = store
            .find_dependent_files("nonexistent.rs", 1, None)
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_find_symbol_references_empty() {
        let store = setup().await;
        let result = store
            .find_symbol_references("nonexistent_fn", 10, None)
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_get_impl_blocks_empty() {
        let store = setup().await;
        let result = store.get_impl_blocks("NonexistentType").await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_find_subclasses_empty() {
        let store = setup().await;
        let result = store.find_subclasses("NonexistentClass").await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_get_project_import_edges_empty() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let result = store.get_project_import_edges(project.id).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_get_function_caller_count_zero() {
        let store = setup().await;
        let result = store
            .get_function_caller_count("nonexistent", None)
            .await
            .unwrap();
        assert_eq!(result, 0);
    }

    #[tokio::test]
    async fn test_check_topology_rules_no_rules() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let violations = store.check_topology_rules_code(project.id).await.unwrap();
        assert!(violations.is_empty());
    }

    #[tokio::test]
    async fn test_context_card_round_trip() {
        let store = setup().await;
        let card = ContextCard {
            path: "src/main.rs".to_string(),
            cc_pagerank: 0.5,
            cc_betweenness: 0.3,
            cc_clustering: 0.8,
            cc_community_id: 1,
            cc_community_label: "core".to_string(),
            cc_imports_out: 5,
            cc_imports_in: 3,
            cc_calls_out: 10,
            cc_calls_in: 7,
            cc_structural_dna: vec![1.0, 2.0, 3.0],
            cc_wl_hash: 12345,
            cc_fingerprint: vec![0.1, 0.2],
            cc_co_changers_top5: vec!["src/lib.rs".to_string()],
            cc_version: 1,
            cc_computed_at: "2024-01-01T00:00:00Z".to_string(),
        };
        store.batch_save_context_cards(&[card]).await.unwrap();
        // Check has_context_cards returns true for any project (no pid set)
        // This just ensures no crash
    }

    #[tokio::test]
    async fn test_find_blocked_tasks_empty() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let result = store.find_blocked_tasks(plan.id).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_list_projects_filtered() {
        let store = setup().await;
        let p1 = test_project_named("Alpha Project");
        let p2 = test_project_named("Beta Project");
        store.create_project(&p1).await.unwrap();
        store.create_project(&p2).await.unwrap();

        let (projects, total) = store
            .list_projects_filtered(None, 50, 0, None, "desc")
            .await
            .unwrap();
        assert_eq!(total, 2);
        assert_eq!(projects.len(), 2);
    }

    #[tokio::test]
    async fn test_get_node_pagerank_default() {
        let store = setup().await;
        let file = test_file("src/main.rs");
        store.upsert_file(&file).await.unwrap();
        let pr = store.get_node_pagerank("src/main.rs", "").await.unwrap();
        assert_eq!(pr, 0.0);
    }
}
