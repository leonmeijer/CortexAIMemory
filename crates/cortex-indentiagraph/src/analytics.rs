//! Graph analytics operations for IndentiaGraphStore.
//!
//! Implements batch analytics updates, community/node queries, code exploration
//! (language stats, connected files, health report, circular dependencies),
//! topology rules, analysis profiles, and process detection.

use anyhow::{Context, Result};
use cortex_core::graph::{
    AnalysisProfile, FileAnalyticsUpdate, FileSignalRecord, FunctionAnalyticsUpdate,
    LinkPrediction, StructuralFingerprintUpdate, TopologyRule, TopologyRuleType, TopologySeverity,
};
use cortex_core::models::{
    BridgeFile, CodeHealthReport, CommunityRow, ConnectedFileNode, CouplingMetrics, GodFunction,
    LanguageStatsNode, NodeAnalyticsRow, NodeGdsMetrics, ProcessNode, ProjectPercentiles,
};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::IndentiaGraphStore;

// ============================================================================
// Helpers for building SurrealDB inline object literals
// ============================================================================

/// Convert SurrealDB's object-to-string format to JSON.
///
/// SurrealDB `<string>` cast of objects produces SurrealQL format:
/// `{ CALLS: 2, IMPORTS: 1.5f }` or `{ 'CALLS': 2f, 'IMPORTS': 1.5f }`
/// We convert to valid JSON: `{"CALLS": 2, "IMPORTS": 1.5}`
fn surreal_object_to_json(s: &str) -> String {
    let mut result = s.to_string();

    // 1. Replace single quotes with double quotes.
    result = result.replace('\'', "\"");

    // 2. Remove trailing 'f' from float/int literals (e.g. "2f" -> "2", "1.5f" -> "1.5").
    if regex::Regex::new(r"(\d+(?:\.\d+)?)f(?:\s*[,}])").is_ok() {
        // Use a lookahead-like approach: capture the number and the delimiter.
        result = regex::Regex::new(r"(\d+(?:\.\d+)?)f(\s*[,}])")
            .map(|re| re.replace_all(&result, "$1$2").to_string())
            .unwrap_or(result);
    }

    // 3. Quote unquoted bare keys (e.g. `CALLS:` -> `"CALLS":`).
    if let Ok(re) = regex::Regex::new(r#"([{,]\s*)([a-zA-Z_][a-zA-Z0-9_]*)(\s*:)"#) {
        result = re.replace_all(&result, r#"$1"$2"$3"#).to_string();
    }

    result
}

/// Build a SurrealDB object literal string from a HashMap<String, f64>.
/// E.g., `IMPORTS: 1.5, CALLS: 2.0`
///
/// Keys are restricted to alphanumeric + underscore characters to prevent
/// injection via malicious key names.
fn build_surreal_object_literal(map: &std::collections::HashMap<String, f64>) -> String {
    map.iter()
        .filter_map(|(k, v)| {
            // Only allow keys that are safe identifiers (alphanumeric + underscore)
            if k.chars().all(|c| c.is_alphanumeric() || c == '_') {
                Some(format!("'{k}': {v}"))
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build a SurrealDB object literal string from a serde_json::Value.
///
/// Keys are restricted to alphanumeric + underscore characters to prevent
/// injection. String values are escaped.
fn build_surreal_object_from_value(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Object(map) => map
            .iter()
            .filter_map(|(k, v)| {
                // Only allow safe identifier keys
                if !k.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return None;
                }
                let val_str = match v {
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::String(s) => {
                        // Escape any single quotes in the string value
                        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
                    }
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => "NONE".to_string(),
                };
                Some(format!("'{k}': {val_str}"))
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

// ============================================================================
// SurrealDB record types for deserialization
// ============================================================================

/// Language + file count aggregation.
#[derive(Debug, SurrealValue)]
struct LangCountRecord {
    language: Option<String>,
    count: Option<i64>,
}

/// File analytics record (read-back after batch write).
#[derive(Debug, SurrealValue)]
struct FileAnalyticsRecord {
    path: Option<String>,
    pagerank: Option<f64>,
    betweenness: Option<f64>,
    clustering_coeff: Option<f64>,
    community_id: Option<i64>,
    wl_hash: Option<String>,
}

/// Simple path-only record.
#[derive(Debug, SurrealValue)]
struct PathOnlyRecord {
    path: Option<String>,
}

/// Community aggregation record.
#[derive(Debug, SurrealValue)]
struct CommunityRecord {
    community_id: Option<i64>,
    file_count: Option<i64>,
}

/// God function candidate record.
#[derive(Debug, SurrealValue)]
struct GodFuncRecord {
    name: Option<String>,
    file_path: Option<String>,
    in_degree: Option<i64>,
    out_degree: Option<i64>,
}

/// File fingerprint record for structural fingerprints.
#[derive(Debug, SurrealValue)]
struct FingerprintRecord {
    path: Option<String>,
    fingerprint: Option<Vec<f64>>,
}

/// File structural DNA record.
#[derive(Debug, SurrealValue)]
struct StructuralDnaRecord {
    path: Option<String>,
    structural_dna: Option<Vec<f64>>,
}

/// File signal record for multi-signal similarity.
#[derive(Debug, SurrealValue)]
struct SignalRecord {
    path: Option<String>,
    fingerprint: Option<Vec<f64>>,
    wl_hash: Option<String>,
    function_count: Option<i64>,
}

/// Import edge record for circular dependency detection.
#[derive(Debug, SurrealValue)]
struct ImportEdgeRecord {
    from_path: Option<String>,
    to_path: Option<String>,
}

/// Topology rule record from SurrealDB.
#[derive(Debug, SurrealValue)]
struct TopologyRuleRecord {
    id: RecordId,
    project_id: Option<String>,
    rule_type: Option<String>,
    source_pattern: Option<String>,
    target_pattern: Option<String>,
    max_value: Option<i64>,
    description: Option<String>,
    created_at: Option<String>,
}

/// Analysis profile record from SurrealDB (without FLEXIBLE object fields).
/// Used for queries where edge_weights/fusion_weights are not needed.
#[derive(Debug, SurrealValue)]
struct AnalysisProfileRecord {
    id: RecordId,
    name: Option<String>,
    description: Option<String>,
    project_id: Option<String>,
    created_at: Option<String>,
}

/// Process record from SurrealDB.
#[derive(Debug, SurrealValue)]
struct ProcessRecord {
    id: RecordId,
    name: Option<String>,
    project_id: Option<String>,
    entry_point: Option<String>,
    file_count: Option<i64>,
    function_count: Option<i64>,
}

/// Count result record.
#[derive(Debug, SurrealValue)]
struct CountRecord {
    count: Option<i64>,
}

/// Clustering coefficient record.
#[derive(Debug, SurrealValue)]
struct ClusteringRecord {
    path: Option<String>,
    clustering_coeff: Option<f64>,
}

impl IndentiaGraphStore {
    // ========================================================================
    // Analytics Updates (4 methods)
    // ========================================================================

    /// Batch-write pagerank, betweenness, community_id, clustering_coeff, and wl_hash
    /// to File nodes.
    pub async fn batch_update_file_analytics(&self, updates: &[FileAnalyticsUpdate]) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        for chunk in updates.chunks(100) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPDATE $rid_{i} SET \
                     pagerank = $pr_{i}, \
                     betweenness = $bt_{i}, \
                     clustering_coeff = $cc_{i}, \
                     community_id = $cid_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, update) in chunk.iter().enumerate() {
                q = q
                    .bind((
                        format!("rid_{i}"),
                        RecordId::new("file", update.path.as_str()),
                    ))
                    .bind((format!("pr_{i}"), update.pagerank))
                    .bind((format!("bt_{i}"), update.betweenness))
                    .bind((format!("cc_{i}"), update.clustering_coefficient))
                    .bind((format!("cid_{i}"), update.community_id as i64));
            }
            q.await.context("Failed to batch update file analytics")?;
        }
        Ok(())
    }

    /// Batch-write pagerank, betweenness, community_id to Function nodes.
    pub async fn batch_update_function_analytics(
        &self,
        updates: &[FunctionAnalyticsUpdate],
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        for chunk in updates.chunks(100) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPDATE $rid_{i} SET \
                     pagerank = $pr_{i}, \
                     betweenness = $bt_{i}, \
                     community_id = $cid_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, update) in chunk.iter().enumerate() {
                q = q
                    .bind((
                        format!("rid_{i}"),
                        RecordId::new("function", update.id.as_str()),
                    ))
                    .bind((format!("pr_{i}"), update.pagerank))
                    .bind((format!("bt_{i}"), update.betweenness))
                    .bind((format!("cid_{i}"), update.community_id as i64));
            }
            q.await
                .context("Failed to batch update function analytics")?;
        }
        Ok(())
    }

    /// Batch-write structural fingerprint vectors (17-dim) to File nodes.
    pub async fn batch_update_structural_fingerprints(
        &self,
        updates: &[StructuralFingerprintUpdate],
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        for chunk in updates.chunks(100) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPDATE $rid_{i} SET fingerprint = $fp_{i} RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, update) in chunk.iter().enumerate() {
                q = q
                    .bind((
                        format!("rid_{i}"),
                        RecordId::new("file", update.path.as_str()),
                    ))
                    .bind((format!("fp_{i}"), update.fingerprint.clone()));
            }
            q.await
                .context("Failed to batch update structural fingerprints")?;
        }
        Ok(())
    }

    /// Write predicted missing links as PREDICTED_LINK relationships.
    pub async fn write_predicted_links(
        &self,
        project_id: &str,
        links: &[LinkPrediction],
    ) -> Result<()> {
        if links.is_empty() {
            return Ok(());
        }

        // Delete existing predicted links for this project first.
        self.db
            .query("DELETE predicted_link WHERE project_id = $pid")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to delete old predicted links")?;

        // Create new predicted links in batches.
        for chunk in links.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "RELATE $src_{i}->predicted_link->$tgt_{i} \
                     SET plausibility = $pl_{i}, project_id = $pid_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, link) in chunk.iter().enumerate() {
                q = q
                    .bind((
                        format!("src_{i}"),
                        RecordId::new("file", link.source.as_str()),
                    ))
                    .bind((
                        format!("tgt_{i}"),
                        RecordId::new("file", link.target.as_str()),
                    ))
                    .bind((format!("pl_{i}"), link.plausibility))
                    .bind((format!("pid_{i}"), project_id.to_string()));
            }
            q.await.context("Failed to write predicted links")?;
        }
        Ok(())
    }

    // ========================================================================
    // Analytics Queries (10 methods)
    // ========================================================================

    /// Get distinct communities for a project (from Louvain clustering).
    pub async fn get_project_communities(&self, project_id: Uuid) -> Result<Vec<CommunityRow>> {
        let pid = project_id.to_string();

        let mut resp = self
            .db
            .query(
                "SELECT community_id, count() AS file_count \
                 FROM `file` \
                 WHERE project_id = $pid AND community_id IS NOT NONE \
                 GROUP BY community_id \
                 ORDER BY file_count DESC",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to get project communities")?;

        let records: Vec<CommunityRecord> = resp.take(0)?;

        let mut communities = Vec::new();
        for r in records {
            let cid = match r.community_id {
                Some(c) => c,
                None => continue,
            };

            // Get top files for this community by pagerank.
            let mut detail_resp = self
                .db
                .query(
                    "SELECT path, pagerank FROM `file` \
                     WHERE project_id = $pid AND community_id = $cid \
                     ORDER BY pagerank DESC \
                     LIMIT 3",
                )
                .bind(("pid", pid.clone()))
                .bind(("cid", cid))
                .await
                .context("Failed to get community key files")?;

            #[derive(Debug, SurrealValue)]
            struct KeyFileRecord {
                path: Option<String>,
                pagerank: Option<f64>,
            }
            let key_files_records: Vec<KeyFileRecord> = detail_resp.take(0)?;
            let key_files: Vec<String> = key_files_records
                .into_iter()
                .filter_map(|r| r.path)
                .collect();

            communities.push(CommunityRow {
                community_id: cid,
                community_label: format!("Community {}", cid),
                file_count: r.file_count.unwrap_or(0) as usize,
                key_files,
                unique_fingerprints: 0, // Not tracked at SurrealDB level
            });
        }

        Ok(communities)
    }

    /// Get analytics properties for a node (File by path, or Function by name).
    pub async fn get_node_analytics(
        &self,
        identifier: &str,
        node_type: &str,
    ) -> Result<Option<NodeAnalyticsRow>> {
        let (table, key) = match node_type {
            "function" => ("function", identifier),
            _ => ("file", identifier),
        };

        let mut resp = self
            .db
            .query(format!(
                "SELECT pagerank, betweenness, community_id \
                 FROM `{table}` \
                 WHERE id = $rid"
            ))
            .bind(("rid", RecordId::new(table, key)))
            .await
            .context("Failed to get node analytics")?;

        let records: Vec<FileAnalyticsRecord> = resp.take(0)?;
        Ok(records.into_iter().next().map(|r| NodeAnalyticsRow {
            pagerank: r.pagerank,
            betweenness: r.betweenness,
            community_id: r.community_id,
            community_label: r.community_id.map(|c| format!("Community {}", c)),
        }))
    }

    /// Get GDS metrics for a specific node in a project.
    pub async fn get_node_gds_metrics(
        &self,
        node_path: &str,
        node_type: &str,
        _project_id: Uuid,
    ) -> Result<Option<NodeGdsMetrics>> {
        let table = if node_type == "function" {
            "function"
        } else {
            "file"
        };

        let mut resp = self
            .db
            .query(format!(
                "SELECT path, pagerank, betweenness, clustering_coeff, community_id \
                 FROM `{table}` WHERE id = $rid"
            ))
            .bind(("rid", RecordId::new(table, node_path)))
            .await
            .context("Failed to get node GDS metrics")?;

        let records: Vec<FileAnalyticsRecord> = resp.take(0)?;
        let r = match records.into_iter().next() {
            Some(r) => r,
            None => return Ok(None),
        };

        // Count in-degree and out-degree from imports.
        let edge_table = if table == "file" { "imports" } else { "calls" };

        let mut in_resp = self
            .db
            .query(format!(
                "SELECT count() AS count FROM `{edge_table}` WHERE out = $rid"
            ))
            .bind(("rid", RecordId::new(table, node_path)))
            .await
            .context("Failed to count in-degree")?;

        let in_counts: Vec<CountRecord> = in_resp.take(0)?;
        let in_degree = in_counts
            .into_iter()
            .next()
            .and_then(|c| c.count)
            .unwrap_or(0);

        let mut out_resp = self
            .db
            .query(format!(
                "SELECT count() AS count FROM `{edge_table}` WHERE in = $rid"
            ))
            .bind(("rid", RecordId::new(table, node_path)))
            .await
            .context("Failed to count out-degree")?;

        let out_counts: Vec<CountRecord> = out_resp.take(0)?;
        let out_degree = out_counts
            .into_iter()
            .next()
            .and_then(|c| c.count)
            .unwrap_or(0);

        Ok(Some(NodeGdsMetrics {
            node_path: node_path.to_string(),
            node_type: node_type.to_string(),
            pagerank: r.pagerank,
            betweenness: r.betweenness,
            clustering_coefficient: r.clustering_coeff,
            community_id: r.community_id,
            community_label: r.community_id.map(|c| format!("Community {}", c)),
            in_degree,
            out_degree,
            fabric_pagerank: None,
            fabric_betweenness: None,
            fabric_community_id: None,
            fabric_community_label: None,
        }))
    }

    /// Get statistical percentiles for analytics scores across all files in a project.
    pub async fn get_project_percentiles(&self, project_id: Uuid) -> Result<ProjectPercentiles> {
        let pid = project_id.to_string();

        // Collect all pagerank and betweenness values.
        #[derive(Debug, SurrealValue)]
        struct MetricsRecord {
            pagerank: Option<f64>,
            betweenness: Option<f64>,
        }

        let mut resp = self
            .db
            .query(
                "SELECT pagerank, betweenness \
                 FROM `file` \
                 WHERE project_id = $pid AND pagerank IS NOT NONE",
            )
            .bind(("pid", pid))
            .await
            .context("Failed to get project percentiles")?;

        let records: Vec<MetricsRecord> = resp.take(0)?;

        let mut pageranks: Vec<f64> = records.iter().filter_map(|r| r.pagerank).collect();
        let mut betweennesses: Vec<f64> = records.iter().filter_map(|r| r.betweenness).collect();

        pageranks.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        betweennesses.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        fn percentile(sorted: &[f64], p: f64) -> f64 {
            if sorted.is_empty() {
                return 0.0;
            }
            let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
            sorted[idx.min(sorted.len() - 1)]
        }

        fn mean(vals: &[f64]) -> f64 {
            if vals.is_empty() {
                return 0.0;
            }
            vals.iter().sum::<f64>() / vals.len() as f64
        }

        fn stddev(vals: &[f64]) -> f64 {
            if vals.len() < 2 {
                return 0.0;
            }
            let m = mean(vals);
            let variance =
                vals.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (vals.len() - 1) as f64;
            variance.sqrt()
        }

        Ok(ProjectPercentiles {
            pagerank_p50: percentile(&pageranks, 0.5),
            pagerank_p80: percentile(&pageranks, 0.8),
            pagerank_p95: percentile(&pageranks, 0.95),
            betweenness_p50: percentile(&betweennesses, 0.5),
            betweenness_p80: percentile(&betweennesses, 0.8),
            betweenness_p95: percentile(&betweennesses, 0.95),
            betweenness_mean: mean(&betweennesses),
            betweenness_stddev: stddev(&betweennesses),
        })
    }

    /// Get top N files by betweenness centrality (bridge files).
    pub async fn get_top_bridges_by_betweenness(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> Result<Vec<BridgeFile>> {
        let pid = project_id.to_string();

        #[derive(Debug, SurrealValue)]
        struct BridgeRecord {
            path: Option<String>,
            betweenness: Option<f64>,
            community_id: Option<i64>,
        }

        let mut resp = self
            .db
            .query(
                "SELECT path, betweenness, community_id \
                 FROM `file` \
                 WHERE project_id = $pid AND betweenness IS NOT NONE \
                 ORDER BY betweenness DESC \
                 LIMIT $lim",
            )
            .bind(("pid", pid))
            .bind(("lim", limit as i64))
            .await
            .context("Failed to get top bridges by betweenness")?;

        let records: Vec<BridgeRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                Some(BridgeFile {
                    path: r.path?,
                    betweenness: r.betweenness.unwrap_or(0.0),
                    community_label: r.community_id.map(|c| format!("Community {}", c)),
                })
            })
            .collect())
    }

    /// Get distinct community labels for a list of file paths.
    pub async fn get_affected_communities(&self, file_paths: &[String]) -> Result<Vec<String>> {
        if file_paths.is_empty() {
            return Ok(vec![]);
        }

        let mut communities = std::collections::HashSet::new();
        for path in file_paths {
            let mut resp = self
                .db
                .query("SELECT community_id FROM `file` WHERE id = $rid")
                .bind(("rid", RecordId::new("file", path.as_str())))
                .await
                .context("Failed to get community for file")?;

            let records: Vec<CommunityRecord> = resp.take(0)?;
            for r in records {
                if let Some(cid) = r.community_id {
                    communities.insert(format!("Community {}", cid));
                }
            }
        }

        Ok(communities.into_iter().collect())
    }

    /// Read structural DNA vectors for all File nodes in a project.
    pub async fn get_project_structural_dna(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, Vec<f64>)>> {
        let mut resp = self
            .db
            .query(
                "SELECT path, structural_dna \
                 FROM `file` \
                 WHERE project_id = $pid AND structural_dna IS NOT NONE",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to get project structural DNA")?;

        let records: Vec<StructuralDnaRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| Some((r.path?, r.structural_dna?)))
            .collect())
    }

    /// Read structural fingerprint vectors for all File nodes in a project.
    pub async fn get_project_structural_fingerprints(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, Vec<f64>)>> {
        let mut resp = self
            .db
            .query(
                "SELECT path, fingerprint \
                 FROM `file` \
                 WHERE project_id = $pid AND fingerprint IS NOT NONE",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to get project structural fingerprints")?;

        let records: Vec<FingerprintRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| Some((r.path?, r.fingerprint?)))
            .collect())
    }

    /// Read all file signals needed for multi-signal structural similarity.
    pub async fn get_project_file_signals(
        &self,
        project_id: &str,
    ) -> Result<Vec<FileSignalRecord>> {
        let mut resp = self
            .db
            .query(
                "SELECT path, fingerprint, wl_hash \
                 FROM `file` \
                 WHERE project_id = $pid AND fingerprint IS NOT NONE",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to get project file signals")?;

        let records: Vec<SignalRecord> = resp.take(0)?;

        // Count functions per file.
        let mut func_resp = self
            .db
            .query(
                "SELECT file_path AS path, count() AS function_count \
                 FROM `function` \
                 WHERE file_path IN \
                    (SELECT VALUE path FROM `file` WHERE project_id = $pid) \
                 GROUP BY file_path",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to count functions per file")?;

        #[derive(Debug, SurrealValue)]
        struct FuncCountRecord {
            path: Option<String>,
            function_count: Option<i64>,
        }

        let func_counts: Vec<FuncCountRecord> = func_resp.take(0)?;
        let func_map: std::collections::HashMap<String, usize> = func_counts
            .into_iter()
            .filter_map(|r| Some((r.path?, r.function_count.unwrap_or(0) as usize)))
            .collect();

        Ok(records
            .into_iter()
            .filter_map(|r| {
                let path = r.path?;
                let fingerprint = r.fingerprint?;
                let wl_hash = r
                    .wl_hash
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                let function_count = func_map.get(&path).copied().unwrap_or(0);
                Some(FileSignalRecord {
                    path,
                    fingerprint,
                    wl_hash,
                    function_count,
                })
            })
            .collect())
    }

    /// Detect circular dependencies in import graph via DFS cycle detection.
    pub async fn get_circular_dependencies(&self, project_id: Uuid) -> Result<Vec<Vec<String>>> {
        let pid = project_id.to_string();

        // Get all import edges for files in the project.
        let mut resp = self
            .db
            .query(
                "SELECT in.path AS from_path, out.path AS to_path \
                 FROM imports \
                 WHERE in.project_id = $pid",
            )
            .bind(("pid", pid))
            .await
            .context("Failed to get import edges for cycle detection")?;

        let edges: Vec<ImportEdgeRecord> = resp.take(0)?;

        // Build adjacency list.
        let mut graph: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for edge in &edges {
            if let (Some(from), Some(to)) = (&edge.from_path, &edge.to_path) {
                graph.entry(from.clone()).or_default().push(to.clone());
            }
        }

        // DFS cycle detection.
        let mut visited = std::collections::HashSet::new();
        let mut rec_stack = std::collections::HashSet::new();
        let mut cycles: Vec<Vec<String>> = Vec::new();
        let mut path: Vec<String> = Vec::new();

        fn dfs(
            node: &str,
            graph: &std::collections::HashMap<String, Vec<String>>,
            visited: &mut std::collections::HashSet<String>,
            rec_stack: &mut std::collections::HashSet<String>,
            path: &mut Vec<String>,
            cycles: &mut Vec<Vec<String>>,
        ) {
            visited.insert(node.to_string());
            rec_stack.insert(node.to_string());
            path.push(node.to_string());

            if let Some(neighbors) = graph.get(node) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        dfs(neighbor, graph, visited, rec_stack, path, cycles);
                    } else if rec_stack.contains(neighbor) {
                        // Found a cycle: extract it from path.
                        if let Some(start_idx) = path.iter().position(|n| n == neighbor) {
                            let cycle: Vec<String> = path[start_idx..].to_vec();
                            if cycle.len() >= 2 {
                                cycles.push(cycle);
                            }
                        }
                    }
                }
            }

            path.pop();
            rec_stack.remove(node);
        }

        let nodes: Vec<String> = graph.keys().cloned().collect();
        for node in &nodes {
            if !visited.contains(node) {
                dfs(
                    node,
                    &graph,
                    &mut visited,
                    &mut rec_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        Ok(cycles)
    }

    // ========================================================================
    // Code Exploration (6 methods)
    // ========================================================================

    /// Get language statistics across all files.
    pub async fn get_language_stats(&self) -> Result<Vec<LanguageStatsNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT language, count() AS count \
                 FROM `file` \
                 GROUP BY language \
                 ORDER BY count DESC",
            )
            .await
            .context("Failed to get language stats")?;

        let records: Vec<LangCountRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                Some(LanguageStatsNode {
                    language: r.language?,
                    file_count: r.count.unwrap_or(0) as usize,
                })
            })
            .collect())
    }

    /// Get language statistics for a specific project.
    pub async fn get_language_stats_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<LanguageStatsNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT language, count() AS count \
                 FROM `file` \
                 WHERE project_id = $pid \
                 GROUP BY language \
                 ORDER BY count DESC",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to get language stats for project")?;

        let records: Vec<LangCountRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                Some(LanguageStatsNode {
                    language: r.language?,
                    file_count: r.count.unwrap_or(0) as usize,
                })
            })
            .collect())
    }

    /// Get most connected files (paths only, highest import in-degree).
    pub async fn get_most_connected_files(&self, limit: usize) -> Result<Vec<String>> {
        // Count how many import edges point TO each file (dependents).
        let mut resp = self
            .db
            .query(
                "SELECT out.path AS path, count() AS dependents \
                 FROM imports \
                 GROUP BY out.path \
                 ORDER BY dependents DESC \
                 LIMIT $lim",
            )
            .bind(("lim", limit as i64))
            .await
            .context("Failed to get most connected files")?;

        #[derive(Debug, SurrealValue)]
        struct ConnPathRecord {
            path: Option<String>,
            dependents: Option<i64>,
        }

        let records: Vec<ConnPathRecord> = resp.take(0)?;
        Ok(records.into_iter().filter_map(|r| r.path).collect())
    }

    /// Get most connected files with import/dependent counts (global).
    pub async fn get_most_connected_files_detailed(
        &self,
        limit: usize,
    ) -> Result<Vec<ConnectedFileNode>> {
        // Dependents (in-degree): other files that IMPORT this file.
        let mut dep_resp = self
            .db
            .query(
                "SELECT out.path AS path, count() AS dependents \
                 FROM imports \
                 GROUP BY out.path",
            )
            .await
            .context("Failed to count dependents")?;

        #[derive(Debug, SurrealValue)]
        struct DepRecord {
            path: Option<String>,
            dependents: Option<i64>,
        }

        let dep_records: Vec<DepRecord> = dep_resp.take(0)?;
        let mut dep_map: std::collections::HashMap<String, i64> = dep_records
            .into_iter()
            .filter_map(|r| Some((r.path?, r.dependents.unwrap_or(0))))
            .collect();

        // Imports (out-degree): files this file imports.
        let mut imp_resp = self
            .db
            .query(
                "SELECT in.path AS path, count() AS imports \
                 FROM imports \
                 GROUP BY in.path",
            )
            .await
            .context("Failed to count imports")?;

        #[derive(Debug, SurrealValue)]
        struct ImpRecord {
            path: Option<String>,
            imports: Option<i64>,
        }

        let imp_records: Vec<ImpRecord> = imp_resp.take(0)?;
        let mut imp_map: std::collections::HashMap<String, i64> = imp_records
            .into_iter()
            .filter_map(|r| Some((r.path?, r.imports.unwrap_or(0))))
            .collect();

        // Merge all known paths.
        let all_paths: std::collections::HashSet<String> =
            dep_map.keys().chain(imp_map.keys()).cloned().collect();

        let mut results: Vec<ConnectedFileNode> = all_paths
            .into_iter()
            .map(|path| {
                let imports = imp_map.remove(&path).unwrap_or(0);
                let dependents = dep_map.remove(&path).unwrap_or(0);
                ConnectedFileNode {
                    path,
                    imports,
                    dependents,
                    pagerank: None,
                    betweenness: None,
                    community_label: None,
                    community_id: None,
                }
            })
            .collect();

        results.sort_by(|a, b| (b.imports + b.dependents).cmp(&(a.imports + a.dependents)));
        results.truncate(limit);
        Ok(results)
    }

    /// Get most connected files for a specific project.
    pub async fn get_most_connected_files_for_project(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> Result<Vec<ConnectedFileNode>> {
        let pid = project_id.to_string();

        // Dependents (how many files import this file).
        let mut dep_resp = self
            .db
            .query(
                "SELECT out.path AS path, count() AS dependents \
                 FROM imports \
                 WHERE out.project_id = $pid \
                 GROUP BY out.path",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to count project dependents")?;

        #[derive(Debug, SurrealValue)]
        struct DepRecord {
            path: Option<String>,
            dependents: Option<i64>,
        }

        let dep_records: Vec<DepRecord> = dep_resp.take(0)?;
        let mut dep_map: std::collections::HashMap<String, i64> = dep_records
            .into_iter()
            .filter_map(|r| Some((r.path?, r.dependents.unwrap_or(0))))
            .collect();

        // Imports (how many files this file imports).
        let mut imp_resp = self
            .db
            .query(
                "SELECT in.path AS path, count() AS imports \
                 FROM imports \
                 WHERE in.project_id = $pid \
                 GROUP BY in.path",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to count project imports")?;

        #[derive(Debug, SurrealValue)]
        struct ImpRecord {
            path: Option<String>,
            imports: Option<i64>,
        }

        let imp_records: Vec<ImpRecord> = imp_resp.take(0)?;
        let mut imp_map: std::collections::HashMap<String, i64> = imp_records
            .into_iter()
            .filter_map(|r| Some((r.path?, r.imports.unwrap_or(0))))
            .collect();

        // Get analytics for all project files.
        let mut analytics_resp = self
            .db
            .query(
                "SELECT path, pagerank, betweenness, community_id \
                 FROM `file` \
                 WHERE project_id = $pid",
            )
            .bind(("pid", pid))
            .await
            .context("Failed to get project file analytics")?;

        let analytics_records: Vec<FileAnalyticsRecord> = analytics_resp.take(0)?;
        let analytics_map: std::collections::HashMap<
            String,
            (Option<f64>, Option<f64>, Option<i64>),
        > = analytics_records
            .into_iter()
            .filter_map(|r| Some((r.path?, (r.pagerank, r.betweenness, r.community_id))))
            .collect();

        let all_paths: std::collections::HashSet<String> = dep_map
            .keys()
            .chain(imp_map.keys())
            .chain(analytics_map.keys())
            .cloned()
            .collect();

        let mut results: Vec<ConnectedFileNode> = all_paths
            .into_iter()
            .map(|path| {
                let imports = imp_map.remove(&path).unwrap_or(0);
                let dependents = dep_map.remove(&path).unwrap_or(0);
                let (pagerank, betweenness, community_id) = analytics_map
                    .get(&path)
                    .copied()
                    .unwrap_or((None, None, None));
                ConnectedFileNode {
                    path,
                    imports,
                    dependents,
                    pagerank,
                    betweenness,
                    community_label: community_id.map(|c| format!("Community {}", c)),
                    community_id,
                }
            })
            .collect();

        results.sort_by(|a, b| (b.imports + b.dependents).cmp(&(a.imports + a.dependents)));
        results.truncate(limit);
        Ok(results)
    }

    /// Get a structural health report: god functions, orphan files, coupling metrics.
    pub async fn get_code_health_report(
        &self,
        project_id: Uuid,
        god_function_threshold: usize,
    ) -> Result<CodeHealthReport> {
        let pid = project_id.to_string();

        // God functions: functions with more callers+callees than the threshold.
        // Count callers (in-degree) and callees (out-degree) for each function.
        let mut caller_resp = self
            .db
            .query(
                "SELECT out.name AS name, out.file_path AS file_path, count() AS in_degree \
                 FROM calls \
                 WHERE out.file_path IN \
                    (SELECT VALUE path FROM `file` WHERE project_id = $pid) \
                 GROUP BY out.name, out.file_path",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to count function callers")?;

        let caller_records: Vec<GodFuncRecord> = caller_resp.take(0)?;
        let mut func_degrees: std::collections::HashMap<(String, String), (usize, usize)> =
            std::collections::HashMap::new();

        for r in caller_records {
            if let (Some(name), Some(file_path)) = (r.name, r.file_path) {
                func_degrees.entry((name, file_path)).or_insert((0, 0)).0 =
                    r.in_degree.unwrap_or(0) as usize;
            }
        }

        let mut callee_resp = self
            .db
            .query(
                "SELECT in.name AS name, in.file_path AS file_path, count() AS out_degree \
                 FROM calls \
                 WHERE in.file_path IN \
                    (SELECT VALUE path FROM `file` WHERE project_id = $pid) \
                 GROUP BY in.name, in.file_path",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to count function callees")?;

        let callee_records: Vec<GodFuncRecord> = callee_resp.take(0)?;
        for r in callee_records {
            if let (Some(name), Some(file_path)) = (r.name, r.file_path) {
                func_degrees.entry((name, file_path)).or_insert((0, 0)).1 =
                    r.out_degree.unwrap_or(0) as usize;
            }
        }

        let god_functions: Vec<GodFunction> = func_degrees
            .into_iter()
            .filter(|(_, (i, o))| *i + *o >= god_function_threshold)
            .map(|((name, file), (in_degree, out_degree))| GodFunction {
                name,
                file,
                in_degree,
                out_degree,
            })
            .collect();

        // Orphan files: files with no import relationships (in or out).
        let mut all_files_resp = self
            .db
            .query("SELECT path FROM `file` WHERE project_id = $pid")
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to get all project files")?;

        let all_files: Vec<PathOnlyRecord> = all_files_resp.take(0)?;
        let all_paths: std::collections::HashSet<String> =
            all_files.into_iter().filter_map(|r| r.path).collect();

        // Get files that have at least one import edge (in or out).
        let mut connected_resp = self
            .db
            .query(
                "SELECT in.path AS path FROM imports WHERE in.project_id = $pid \
                 UNION \
                 SELECT out.path AS path FROM imports WHERE out.project_id = $pid",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to get connected files")?;

        let connected_records: Vec<PathOnlyRecord> = connected_resp.take(0)?;
        let connected_paths: std::collections::HashSet<String> = connected_records
            .into_iter()
            .filter_map(|r| r.path)
            .collect();

        let orphan_files: Vec<String> = all_paths.difference(&connected_paths).cloned().collect();

        // Coupling metrics from clustering coefficients.
        let mut cluster_resp = self
            .db
            .query(
                "SELECT path, clustering_coeff \
                 FROM `file` \
                 WHERE project_id = $pid AND clustering_coeff IS NOT NONE",
            )
            .bind(("pid", pid))
            .await
            .context("Failed to get clustering coefficients")?;

        let cluster_records: Vec<ClusteringRecord> = cluster_resp.take(0)?;

        let coupling_metrics = if cluster_records.is_empty() {
            None
        } else {
            let coeffs: Vec<f64> = cluster_records
                .iter()
                .filter_map(|r| r.clustering_coeff)
                .collect();
            let avg = if coeffs.is_empty() {
                0.0
            } else {
                coeffs.iter().sum::<f64>() / coeffs.len() as f64
            };
            let (max_val, max_path) = cluster_records
                .iter()
                .filter_map(|r| Some((r.clustering_coeff?, r.path.as_deref()?)))
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or((0.0, ""));

            Some(CouplingMetrics {
                avg_clustering_coefficient: avg,
                max_clustering_coefficient: max_val,
                most_coupled_file: if max_path.is_empty() {
                    None
                } else {
                    Some(max_path.to_string())
                },
            })
        };

        Ok(CodeHealthReport {
            god_functions,
            orphan_files,
            coupling_metrics,
        })
    }

    // ========================================================================
    // Topology Rules (3 methods)
    // ========================================================================

    /// Create a topology rule.
    pub async fn create_topology_rule(&self, rule: &TopologyRule) -> Result<()> {
        let rid = RecordId::new("topology_rule", rule.id.as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 project_id = $pid, \
                 rule_type = $rt, \
                 source_pattern = $sp, \
                 target_pattern = $tp, \
                 max_value = $mv, \
                 description = $desc, \
                 created_at = $ca",
            )
            .bind(("rid", rid))
            .bind(("pid", rule.project_id.clone()))
            .bind(("rt", rule.rule_type.to_string()))
            .bind(("sp", rule.source_pattern.clone()))
            .bind(("tp", rule.target_pattern.clone()))
            .bind(("mv", rule.threshold.map(|t| t as i64)))
            .bind(("desc", rule.description.clone()))
            .bind(("ca", chrono::Utc::now().to_rfc3339()))
            .await
            .context("Failed to create topology rule")?;

        Ok(())
    }

    /// List all topology rules for a project.
    pub async fn list_topology_rules(&self, project_id: &str) -> Result<Vec<TopologyRule>> {
        let mut resp = self
            .db
            .query("SELECT * FROM topology_rule WHERE project_id = $pid ORDER BY created_at DESC")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to list topology rules")?;

        let records: Vec<TopologyRuleRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                let rule_type_str = r.rule_type?;
                let rule_type = TopologyRuleType::from_str_loose(&rule_type_str)?;

                Some(TopologyRule {
                    id: crate::client::rid_to_uuid(&r.id).ok()?.to_string(),
                    project_id: r.project_id?,
                    rule_type,
                    source_pattern: r.source_pattern?,
                    target_pattern: r.target_pattern,
                    threshold: r.max_value.map(|v| v as u32),
                    severity: TopologySeverity::Error,
                    description: r.description.unwrap_or_default(),
                })
            })
            .collect())
    }

    /// Delete a topology rule by id.
    pub async fn delete_topology_rule(&self, rule_id: &str) -> Result<()> {
        let rid = RecordId::new("topology_rule", rule_id);
        self.db
            .query("DELETE $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to delete topology rule")?;
        Ok(())
    }

    // ========================================================================
    // Analysis Profiles (4 methods)
    // ========================================================================

    /// Create or update an analysis profile.
    ///
    /// The edge_weights and fusion_weights are FLEXIBLE object fields in the schema.
    /// We inline their JSON representation in the SQL to avoid bind type issues.
    pub async fn create_analysis_profile(&self, profile: &AnalysisProfile) -> Result<()> {
        // Build individual key-value SET entries for edge_weights as inline object.
        let ew_entries = build_surreal_object_literal(&profile.edge_weights);
        let fw_obj = serde_json::to_value(&profile.fusion_weights)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        let fw_entries = build_surreal_object_from_value(&fw_obj);

        let query = format!(
            "UPSERT $rid SET \
             name = $name, \
             description = $desc, \
             project_id = $pid, \
             edge_weights = {{ {ew_entries} }}, \
             fusion_weights = {{ {fw_entries} }}, \
             created_at = $ca"
        );

        self.db
            .query(&query)
            .bind((
                "rid",
                RecordId::new("analysis_profile", profile.id.as_str()),
            ))
            .bind(("name", profile.name.clone()))
            .bind(("desc", profile.description.clone()))
            .bind(("pid", profile.project_id.clone()))
            .bind(("ca", chrono::Utc::now().to_rfc3339()))
            .await
            .context("Failed to create analysis profile")?;

        Ok(())
    }

    /// Get a single analysis profile by id.
    pub async fn get_analysis_profile(&self, id: &str) -> Result<Option<AnalysisProfile>> {
        let rid = RecordId::new("analysis_profile", id);

        // Fetch base fields (skip FLEXIBLE object fields that can't be deserialized by SurrealValue).
        let mut resp = self
            .db
            .query("SELECT id, name, description, project_id, created_at FROM $rid")
            .bind(("rid", rid.clone()))
            .await
            .context("Failed to get analysis profile")?;

        let records: Vec<AnalysisProfileRecord> = resp.take(0)?;
        let base = match records.into_iter().next() {
            Some(r) => r,
            None => return Ok(None),
        };

        // Fetch FLEXIBLE object fields separately as string casts.
        let (ew, fw) = self.fetch_profile_weights(&rid).await?;

        Ok(Some(self.build_profile_from_record(base, ew, fw)))
    }

    /// List analysis profiles visible to a project (global + project-specific).
    pub async fn list_analysis_profiles(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<AnalysisProfile>> {
        let mut resp = if let Some(pid) = project_id {
            self.db
                .query(
                    "SELECT id, name, description, project_id, created_at \
                     FROM analysis_profile \
                     WHERE project_id IS NONE OR project_id = $pid \
                     ORDER BY created_at DESC",
                )
                .bind(("pid", pid.to_string()))
                .await
                .context("Failed to list analysis profiles")?
        } else {
            self.db
                .query(
                    "SELECT id, name, description, project_id, created_at \
                     FROM analysis_profile \
                     WHERE project_id IS NONE \
                     ORDER BY created_at DESC",
                )
                .await
                .context("Failed to list global analysis profiles")?
        };

        let records: Vec<AnalysisProfileRecord> = resp.take(0)?;
        let mut profiles = Vec::with_capacity(records.len());
        for base in records {
            let (ew, fw) = self.fetch_profile_weights(&base.id).await?;
            profiles.push(self.build_profile_from_record(base, ew, fw));
        }
        Ok(profiles)
    }

    /// Delete an analysis profile by id.
    pub async fn delete_analysis_profile(&self, id: &str) -> Result<()> {
        let rid = RecordId::new("analysis_profile", id);
        self.db
            .query("DELETE $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to delete analysis profile")?;
        Ok(())
    }

    /// Fetch edge_weights and fusion_weights for a profile as string casts.
    async fn fetch_profile_weights(
        &self,
        rid: &RecordId,
    ) -> Result<(
        std::collections::HashMap<String, f64>,
        cortex_core::graph::FusionWeights,
    )> {
        #[derive(Debug, SurrealValue)]
        struct WeightsRecord {
            ew: Option<String>,
            fw: Option<String>,
        }

        let mut resp = self
            .db
            .query("SELECT <string>edge_weights AS ew, <string>fusion_weights AS fw FROM $rid")
            .bind(("rid", rid.clone()))
            .await
            .context("Failed to fetch profile weights")?;

        let records: Vec<WeightsRecord> = resp.take(0)?;
        let r = records.into_iter().next();

        let edge_weights = r
            .as_ref()
            .and_then(|r| r.ew.as_deref())
            .and_then(|s| {
                let json_str = surreal_object_to_json(s);
                serde_json::from_str(&json_str).ok()
            })
            .unwrap_or_default();

        let fusion_weights = r
            .as_ref()
            .and_then(|r| r.fw.as_deref())
            .and_then(|s| {
                let json_str = surreal_object_to_json(s);
                serde_json::from_str(&json_str).ok()
            })
            .unwrap_or_default();

        Ok((edge_weights, fusion_weights))
    }

    /// Build an AnalysisProfile from a base record + weights.
    fn build_profile_from_record(
        &self,
        r: AnalysisProfileRecord,
        edge_weights: std::collections::HashMap<String, f64>,
        fusion_weights: cortex_core::graph::FusionWeights,
    ) -> AnalysisProfile {
        AnalysisProfile {
            id: crate::client::rid_to_uuid(&r.id)
                .map(|u| u.to_string())
                .unwrap_or_default(),
            project_id: r.project_id,
            name: r.name.unwrap_or_default(),
            description: r.description,
            edge_weights,
            fusion_weights,
            is_builtin: false,
        }
    }

    // ========================================================================
    // Process Detection (3 methods)
    // ========================================================================

    /// Batch upsert Process nodes.
    pub async fn batch_upsert_processes(&self, processes: &[ProcessNode]) -> Result<()> {
        if processes.is_empty() {
            return Ok(());
        }

        for chunk in processes.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "CREATE $rid_{i} SET \
                     name = $name_{i}, \
                     project_id = $pid_{i}, \
                     entry_point = $ep_{i}, \
                     file_count = $fc_{i}, \
                     function_count = $fnc_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, process) in chunk.iter().enumerate() {
                q = q
                    .bind((
                        format!("rid_{i}"),
                        RecordId::new("process", process.id.as_str()),
                    ))
                    .bind((format!("name_{i}"), process.label.clone()))
                    .bind((
                        format!("pid_{i}"),
                        process
                            .project_id
                            .map(|u| u.to_string())
                            .unwrap_or_default(),
                    ))
                    .bind((format!("ep_{i}"), process.entry_point_id.clone()))
                    .bind((format!("fc_{i}"), 0i64)) // Computed externally
                    .bind((format!("fnc_{i}"), process.step_count as i64));
            }
            q.await.context("Failed to batch upsert processes")?;
        }
        Ok(())
    }

    /// Batch create STEP_IN_PROCESS relationships.
    /// Takes (process_id, function_id, step_number) tuples.
    pub async fn batch_create_step_relationships(
        &self,
        steps: &[(String, String, u32)],
    ) -> Result<()> {
        if steps.is_empty() {
            return Ok(());
        }

        for chunk in steps.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "RELATE $func_{i}->step_in_process->$proc_{i} RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, (process_id, function_id, _step)) in chunk.iter().enumerate() {
                q = q
                    .bind((
                        format!("func_{i}"),
                        RecordId::new("function", function_id.as_str()),
                    ))
                    .bind((
                        format!("proc_{i}"),
                        RecordId::new("process", process_id.as_str()),
                    ));
            }
            q.await
                .context("Failed to batch create step relationships")?;
        }
        Ok(())
    }

    /// Delete all Process nodes and their STEP_IN_PROCESS relationships for a project.
    pub async fn delete_project_processes(&self, project_id: Uuid) -> Result<u64> {
        let pid = project_id.to_string();

        // Get process IDs.
        let mut resp = self
            .db
            .query("SELECT id FROM process WHERE project_id = $pid")
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to list project processes for deletion")?;

        let records: Vec<ProcessRecord> = resp.take(0)?;
        let count = records.len() as u64;

        if count == 0 {
            return Ok(0);
        }

        // Delete step_in_process edges for each process.
        for r in &records {
            self.db
                .query("DELETE step_in_process WHERE out = $rid")
                .bind(("rid", r.id.clone()))
                .await
                .context("Failed to delete step_in_process edges")?;
        }

        // Delete process nodes.
        self.db
            .query("DELETE process WHERE project_id = $pid")
            .bind(("pid", pid))
            .await
            .context("Failed to delete project processes")?;

        Ok(count)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::test_helpers::{test_file_for_project, test_function, test_project};

    async fn setup() -> IndentiaGraphStore {
        IndentiaGraphStore::new_memory().await.unwrap()
    }

    async fn setup_with_project() -> (IndentiaGraphStore, Uuid) {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        (store, project.id)
    }

    #[tokio::test]
    async fn test_batch_update_file_analytics() {
        let (store, pid) = setup_with_project().await;

        // Create files.
        let f1 = test_file_for_project("src/main.rs", pid);
        let f2 = test_file_for_project("src/lib.rs", pid);
        store.upsert_file(&f1).await.unwrap();
        store.upsert_file(&f2).await.unwrap();

        // Batch update analytics.
        let updates = vec![
            FileAnalyticsUpdate {
                path: "src/main.rs".to_string(),
                pagerank: 0.85,
                betweenness: 0.42,
                community_id: 1,
                community_label: "Community 1".to_string(),
                clustering_coefficient: 0.75,
                component_id: 0,
            },
            FileAnalyticsUpdate {
                path: "src/lib.rs".to_string(),
                pagerank: 0.55,
                betweenness: 0.22,
                community_id: 2,
                community_label: "Community 2".to_string(),
                clustering_coefficient: 0.50,
                component_id: 0,
            },
        ];
        store.batch_update_file_analytics(&updates).await.unwrap();

        // Read back analytics.
        let analytics = store
            .get_node_analytics("src/main.rs", "file")
            .await
            .unwrap();
        assert!(analytics.is_some());
        let a = analytics.unwrap();
        assert!((a.pagerank.unwrap() - 0.85).abs() < 0.01);
        assert!((a.betweenness.unwrap() - 0.42).abs() < 0.01);
        assert_eq!(a.community_id.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_batch_update_function_analytics() {
        let (store, pid) = setup_with_project().await;

        let f1 = test_file_for_project("src/main.rs", pid);
        store.upsert_file(&f1).await.unwrap();

        let func = test_function("main", "src/main.rs");
        store.upsert_function(&func).await.unwrap();

        // The function key is "file_path::name::line_start".
        let func_id = format!("{}::{}::{}", func.file_path, func.name, func.line_start);

        let updates = vec![FunctionAnalyticsUpdate {
            id: func_id.clone(),
            pagerank: 0.95,
            betweenness: 0.65,
            community_id: 3,
            clustering_coefficient: 0.0,
            component_id: 0,
        }];
        store
            .batch_update_function_analytics(&updates)
            .await
            .unwrap();

        // Read back via get_node_analytics.
        let analytics = store
            .get_node_analytics(&func_id, "function")
            .await
            .unwrap();
        assert!(analytics.is_some());
        let a = analytics.unwrap();
        assert!((a.pagerank.unwrap() - 0.95).abs() < 0.01);
        assert_eq!(a.community_id.unwrap(), 3);
    }

    #[tokio::test]
    async fn test_language_stats() {
        let (store, pid) = setup_with_project().await;

        // Create files with different languages.
        let mut f1 = test_file_for_project("src/main.rs", pid);
        f1.language = "rust".to_string();
        store.upsert_file(&f1).await.unwrap();

        let mut f2 = test_file_for_project("src/lib.rs", pid);
        f2.language = "rust".to_string();
        store.upsert_file(&f2).await.unwrap();

        let mut f3 = test_file_for_project("src/app.ts", pid);
        f3.language = "typescript".to_string();
        store.upsert_file(&f3).await.unwrap();

        // Global stats.
        let stats = store.get_language_stats().await.unwrap();
        assert!(!stats.is_empty());
        let rust_stats = stats.iter().find(|s| s.language == "rust");
        assert!(rust_stats.is_some());
        assert_eq!(rust_stats.unwrap().file_count, 2);

        // Project stats.
        let proj_stats = store.get_language_stats_for_project(pid).await.unwrap();
        assert!(!proj_stats.is_empty());
        let ts_stats = proj_stats.iter().find(|s| s.language == "typescript");
        assert!(ts_stats.is_some());
        assert_eq!(ts_stats.unwrap().file_count, 1);
    }

    #[tokio::test]
    async fn test_most_connected_files() {
        let (store, pid) = setup_with_project().await;

        // Create files.
        let f1 = test_file_for_project("src/main.rs", pid);
        let f2 = test_file_for_project("src/lib.rs", pid);
        let f3 = test_file_for_project("src/utils.rs", pid);
        store.upsert_file(&f1).await.unwrap();
        store.upsert_file(&f2).await.unwrap();
        store.upsert_file(&f3).await.unwrap();

        // Create import relationships: main -> lib, main -> utils, utils -> lib.
        store
            .create_import_relationship("src/main.rs", "src/lib.rs", "crate::lib")
            .await
            .unwrap();
        store
            .create_import_relationship("src/main.rs", "src/utils.rs", "crate::utils")
            .await
            .unwrap();
        store
            .create_import_relationship("src/utils.rs", "src/lib.rs", "crate::lib")
            .await
            .unwrap();

        // lib.rs should be most connected (2 dependents).
        let connected = store.get_most_connected_files(10).await.unwrap();
        assert!(!connected.is_empty());
        // lib.rs should appear first (most dependents).
        assert_eq!(connected[0], "src/lib.rs");

        // Detailed version.
        let detailed = store.get_most_connected_files_detailed(10).await.unwrap();
        assert!(!detailed.is_empty());
        let lib_entry = detailed.iter().find(|f| f.path == "src/lib.rs");
        assert!(lib_entry.is_some());
        assert_eq!(lib_entry.unwrap().dependents, 2);

        // Project-scoped.
        let proj_connected = store
            .get_most_connected_files_for_project(pid, 10)
            .await
            .unwrap();
        assert!(!proj_connected.is_empty());
    }

    #[tokio::test]
    async fn test_communities() {
        let (store, pid) = setup_with_project().await;

        // Create files with community IDs.
        let f1 = test_file_for_project("src/a.rs", pid);
        let f2 = test_file_for_project("src/b.rs", pid);
        let f3 = test_file_for_project("src/c.rs", pid);
        store.upsert_file(&f1).await.unwrap();
        store.upsert_file(&f2).await.unwrap();
        store.upsert_file(&f3).await.unwrap();

        // Assign community IDs.
        let updates = vec![
            FileAnalyticsUpdate {
                path: "src/a.rs".to_string(),
                pagerank: 0.5,
                betweenness: 0.1,
                community_id: 1,
                community_label: "C1".to_string(),
                clustering_coefficient: 0.0,
                component_id: 0,
            },
            FileAnalyticsUpdate {
                path: "src/b.rs".to_string(),
                pagerank: 0.3,
                betweenness: 0.2,
                community_id: 1,
                community_label: "C1".to_string(),
                clustering_coefficient: 0.0,
                component_id: 0,
            },
            FileAnalyticsUpdate {
                path: "src/c.rs".to_string(),
                pagerank: 0.8,
                betweenness: 0.5,
                community_id: 2,
                community_label: "C2".to_string(),
                clustering_coefficient: 0.0,
                component_id: 0,
            },
        ];
        store.batch_update_file_analytics(&updates).await.unwrap();

        // Query communities.
        let communities = store.get_project_communities(pid).await.unwrap();
        assert_eq!(communities.len(), 2);

        // Community 1 should have 2 files, community 2 should have 1.
        let c1 = communities.iter().find(|c| c.community_id == 1);
        assert!(c1.is_some());
        assert_eq!(c1.unwrap().file_count, 2);

        let c2 = communities.iter().find(|c| c.community_id == 2);
        assert!(c2.is_some());
        assert_eq!(c2.unwrap().file_count, 1);

        // Affected communities.
        let affected = store
            .get_affected_communities(&["src/a.rs".to_string(), "src/c.rs".to_string()])
            .await
            .unwrap();
        assert_eq!(affected.len(), 2);
    }

    #[tokio::test]
    async fn test_topology_rules() {
        let (store, pid) = setup_with_project().await;

        let rule_id = Uuid::new_v4();
        let rule = TopologyRule {
            id: rule_id.to_string(),
            project_id: pid.to_string(),
            rule_type: TopologyRuleType::MustNotImport,
            source_pattern: "src/api/**".to_string(),
            target_pattern: Some("src/indentiagraph/**".to_string()),
            threshold: None,
            severity: TopologySeverity::Error,
            description: "API layer must not import IndentiaGraph layer".to_string(),
        };

        // Create.
        store.create_topology_rule(&rule).await.unwrap();

        // List.
        let rules = store.list_topology_rules(&pid.to_string()).await.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].rule_type, TopologyRuleType::MustNotImport);
        assert_eq!(rules[0].source_pattern, "src/api/**");

        // Delete.
        store
            .delete_topology_rule(&rule_id.to_string())
            .await
            .unwrap();
        let rules_after = store.list_topology_rules(&pid.to_string()).await.unwrap();
        assert_eq!(rules_after.len(), 0);
    }

    #[tokio::test]
    async fn test_analysis_profiles() {
        let (store, pid) = setup_with_project().await;

        let profile_id = Uuid::new_v4();
        let profile = AnalysisProfile {
            id: profile_id.to_string(),
            project_id: Some(pid.to_string()),
            name: "security".to_string(),
            description: Some("Security-focused analysis".to_string()),
            edge_weights: std::collections::HashMap::from([
                ("IMPORTS".to_string(), 1.5),
                ("CALLS".to_string(), 2.0),
            ]),
            fusion_weights: Default::default(),
            is_builtin: false,
        };

        // Create.
        store.create_analysis_profile(&profile).await.unwrap();

        // Verify the record exists via raw count.
        let mut count_resp = store
            .db
            .query("SELECT count() AS count FROM analysis_profile")
            .await
            .unwrap();
        let counts: Vec<CountRecord> = count_resp.take(0).unwrap();
        assert_eq!(
            counts.first().and_then(|c| c.count).unwrap_or(0),
            1,
            "analysis_profile should have 1 record after create"
        );

        // Get.
        let fetched = store
            .get_analysis_profile(&profile_id.to_string())
            .await
            .unwrap();
        assert!(
            fetched.is_some(),
            "get_analysis_profile should find the profile"
        );
        let f = fetched.unwrap();
        assert_eq!(f.name, "security");
        assert_eq!(f.edge_weights.get("CALLS"), Some(&2.0));

        // List (project-scoped).
        let profiles = store
            .list_analysis_profiles(Some(&pid.to_string()))
            .await
            .unwrap();
        assert_eq!(profiles.len(), 1);

        // Delete.
        store
            .delete_analysis_profile(&profile_id.to_string())
            .await
            .unwrap();
        let deleted = store
            .get_analysis_profile(&profile_id.to_string())
            .await
            .unwrap();
        assert!(deleted.is_none());
    }

    #[tokio::test]
    async fn test_processes() {
        let (store, pid) = setup_with_project().await;

        let proc_id = Uuid::new_v4().to_string();
        let processes = vec![ProcessNode {
            id: proc_id.clone(),
            label: "Main Request Flow".to_string(),
            process_type: "intra_community".to_string(),
            step_count: 5,
            entry_point_id: "main".to_string(),
            terminal_id: "response".to_string(),
            communities: vec![1],
            project_id: Some(pid),
        }];

        // Batch upsert.
        store.batch_upsert_processes(&processes).await.unwrap();

        // Verify it was created by querying directly.
        let mut resp = store
            .db
            .query("SELECT * FROM process WHERE project_id = $pid")
            .bind(("pid", pid.to_string()))
            .await
            .unwrap();
        let records: Vec<ProcessRecord> = resp.take(0).unwrap();
        assert_eq!(records.len(), 1);

        // Delete.
        let count = store.delete_project_processes(pid).await.unwrap();
        assert_eq!(count, 1);

        // Verify deletion.
        let mut resp2 = store
            .db
            .query("SELECT * FROM process WHERE project_id = $pid")
            .bind(("pid", pid.to_string()))
            .await
            .unwrap();
        let records2: Vec<ProcessRecord> = resp2.take(0).unwrap();
        assert_eq!(records2.len(), 0);
    }
}
