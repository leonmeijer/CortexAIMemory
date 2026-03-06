//! Knowledge Fabric operations for IndentiaGraphStore.
//!
//! Implements co-change computation, churn scores, knowledge density,
//! risk assessment, neural metrics, and batch file analytics updates.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::graph::{FabricFileAnalyticsUpdate, StructuralDnaUpdate};
use cortex_core::models::{
    CoChangePair, CoChanger, FileChurnScore, FileKnowledgeDensity, FileRiskScore, NeuralMetrics,
    RiskFactors,
};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::IndentiaGraphStore;

// ============================================================================
// SurrealDB record types for deserialization
// ============================================================================

/// A pair of file paths from the same commit (for co-change computation).
#[derive(Debug, SurrealValue)]
struct CommitFilePair {
    commit_hash: Option<String>,
    file_path: Option<String>,
}

/// Co-changed edge record.
#[derive(Debug, SurrealValue)]
struct CoChangedRecord {
    file_a: Option<String>,
    file_b: Option<String>,
    count: Option<i64>,
}

/// Co-changer record (single file that co-changes with a target).
#[derive(Debug, SurrealValue)]
struct CoChangerRecord {
    path: Option<String>,
    count: Option<i64>,
}

/// Churn aggregation record from touches query.
#[derive(Debug, SurrealValue)]
struct ChurnRecord {
    path: Option<String>,
    commit_count: Option<i64>,
    total_additions: Option<i64>,
    total_deletions: Option<i64>,
}

/// File analytics record for risk computation.
#[derive(Debug, SurrealValue)]
struct FileAnalyticsRecord {
    path: Option<String>,
    pagerank: Option<f64>,
    betweenness: Option<f64>,
    churn_score: Option<f64>,
    knowledge_density: Option<f64>,
}

/// Neural metrics aggregation record.
#[derive(Debug, SurrealValue)]
struct CountRecord {
    count: i64,
}

/// Average value record.
#[derive(Debug, SurrealValue)]
struct AvgRecord {
    avg_val: Option<f64>,
}

/// Hotspot record (pre-computed churn scores).
#[derive(Debug, SurrealValue)]
struct HotspotRecord {
    path: Option<String>,
    churn_score: Option<f64>,
}

/// Risk summary record.
#[derive(Debug, SurrealValue)]
struct RiskLevelRecord {
    risk_level: Option<String>,
    count: i64,
}

impl IndentiaGraphStore {
    // ========================================================================
    // Co-Change (4 methods)
    // ========================================================================

    /// Compute co-changed pairs from commit touches.
    ///
    /// For each pair of files touched by the same commit, create/update
    /// a `co_changed` edge with the count. Only considers commits that
    /// touch files belonging to the given project.
    pub async fn compute_co_changed(
        &self,
        project_id: Uuid,
        since: Option<DateTime<Utc>>,
        min_count: i64,
        max_relations: i64,
    ) -> Result<i64> {
        let pid = project_id.to_string();

        // Step 1: Find all (commit_hash, file_path) tuples for this project.
        // touches: commit -> file, file has project_id.
        let mut resp = if let Some(since_dt) = since {
            self.db
                .query(
                    "SELECT in.hash AS commit_hash, out.path AS file_path \
                     FROM touches \
                     WHERE out.project_id = $pid \
                       AND in.timestamp >= $since \
                     ORDER BY commit_hash",
                )
                .bind(("pid", pid.clone()))
                .bind(("since", since_dt.to_rfc3339()))
                .await
                .context("Failed to query touches for co-change")?
        } else {
            self.db
                .query(
                    "SELECT in.hash AS commit_hash, out.path AS file_path \
                     FROM touches \
                     WHERE out.project_id = $pid \
                     ORDER BY commit_hash",
                )
                .bind(("pid", pid.clone()))
                .await
                .context("Failed to query touches for co-change")?
        };

        let pairs: Vec<CommitFilePair> = resp.take(0)?;

        // Step 2: Group files by commit.
        let mut commit_files: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for pair in pairs {
            if let (Some(hash), Some(path)) = (pair.commit_hash, pair.file_path) {
                commit_files.entry(hash).or_default().push(path);
            }
        }

        // Step 3: For each commit, generate all file pairs.
        let mut pair_counts: std::collections::HashMap<(String, String), i64> =
            std::collections::HashMap::new();
        for files in commit_files.values() {
            if files.len() < 2 {
                continue;
            }
            let mut sorted = files.clone();
            sorted.sort();
            sorted.dedup();
            for i in 0..sorted.len() {
                for j in (i + 1)..sorted.len() {
                    let key = (sorted[i].clone(), sorted[j].clone());
                    *pair_counts.entry(key).or_insert(0) += 1;
                }
            }
        }

        // Step 4: Filter by min_count and limit to max_relations.
        let mut pairs_vec: Vec<((String, String), i64)> = pair_counts
            .into_iter()
            .filter(|(_, count)| *count >= min_count)
            .collect();
        pairs_vec.sort_by(|a, b| b.1.cmp(&a.1));
        pairs_vec.truncate(max_relations as usize);

        // Step 5: Upsert co_changed edges.
        let mut created = 0i64;
        for chunk in pairs_vec.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "RELATE $from_{i}->co_changed->$to_{i} \
                     SET weight = $w_{i}, count = $c_{i}, project_id = $pid_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, ((file_a, file_b), count)) in chunk.iter().enumerate() {
                let weight = (*count as f64).ln().max(0.1);
                q = q
                    .bind((format!("from_{i}"), RecordId::new("file", file_a.as_str())))
                    .bind((format!("to_{i}"), RecordId::new("file", file_b.as_str())))
                    .bind((format!("w_{i}"), weight))
                    .bind((format!("c_{i}"), *count))
                    .bind((format!("pid_{i}"), pid.clone()));
                created += 1;
            }
            q.await.context("Failed to upsert co_changed edges")?;
        }

        Ok(created)
    }

    /// Update project `last_co_change_computed_at` timestamp to now.
    pub async fn update_project_co_change_timestamp(&self, id: Uuid) -> Result<()> {
        let record_id = RecordId::new("project", id.to_string().as_str());
        let now = Utc::now().to_rfc3339();
        self.db
            .query("UPDATE $record_id SET last_co_change_computed_at = $now RETURN NONE")
            .bind(("record_id", record_id))
            .bind(("now", now))
            .await
            .context("Failed to update co-change timestamp")?;
        Ok(())
    }

    /// Get the co-change graph for a project.
    pub async fn get_co_change_graph(
        &self,
        project_id: Uuid,
        min_count: i64,
        limit: i64,
    ) -> Result<Vec<CoChangePair>> {
        let mut resp = self
            .db
            .query(
                "SELECT in.path AS file_a, out.path AS file_b, count \
                 FROM co_changed \
                 WHERE project_id = $pid AND count >= $min \
                 ORDER BY count DESC \
                 LIMIT $lim",
            )
            .bind(("pid", project_id.to_string()))
            .bind(("min", min_count))
            .bind(("lim", limit))
            .await
            .context("Failed to get co-change graph")?;

        let records: Vec<CoChangedRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                Some(CoChangePair {
                    file_a: r.file_a?,
                    file_b: r.file_b?,
                    count: r.count.unwrap_or(0),
                    last_at: None,
                })
            })
            .collect())
    }

    /// Get files that co-change with a given file.
    pub async fn get_file_co_changers(
        &self,
        file_path: &str,
        min_count: i64,
        limit: i64,
    ) -> Result<Vec<CoChanger>> {
        // co_changed edges: file -> file. Check both directions.
        let file_rid = RecordId::new("file", file_path);

        let mut resp = self
            .db
            .query(
                "SELECT out.path AS path, count \
                 FROM co_changed \
                 WHERE in = $frid AND count >= $min \
                 ORDER BY count DESC \
                 LIMIT $lim",
            )
            .bind(("frid", file_rid.clone()))
            .bind(("min", min_count))
            .bind(("lim", limit))
            .await
            .context("Failed to get file co-changers (outgoing)")?;

        let outgoing: Vec<CoChangerRecord> = resp.take(0)?;

        let mut resp2 = self
            .db
            .query(
                "SELECT in.path AS path, count \
                 FROM co_changed \
                 WHERE out = $frid AND count >= $min \
                 ORDER BY count DESC \
                 LIMIT $lim",
            )
            .bind(("frid", file_rid))
            .bind(("min", min_count))
            .bind(("lim", limit))
            .await
            .context("Failed to get file co-changers (incoming)")?;

        let incoming: Vec<CoChangerRecord> = resp2.take(0)?;

        // Merge results, dedup by path, keep max count.
        let mut merged: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for r in outgoing.into_iter().chain(incoming) {
            if let Some(path) = r.path {
                let count = r.count.unwrap_or(0);
                let entry = merged.entry(path).or_insert(0);
                *entry = (*entry).max(count);
            }
        }

        let mut results: Vec<CoChanger> = merged
            .into_iter()
            .map(|(path, count)| CoChanger {
                path,
                count,
                last_at: None,
            })
            .collect();
        results.sort_by(|a, b| b.count.cmp(&a.count));
        results.truncate(limit as usize);
        Ok(results)
    }

    // ========================================================================
    // Churn (3 methods)
    // ========================================================================

    /// Compute churn metrics per file via TOUCHES relations.
    ///
    /// For each file in the project, counts how many commits touch it,
    /// sums additions + deletions, and counts co_changed edges.
    pub async fn compute_churn_scores(&self, project_id: Uuid) -> Result<Vec<FileChurnScore>> {
        let pid = project_id.to_string();

        // Get touches aggregation per file
        let mut resp = self
            .db
            .query(
                "SELECT out.path AS path, \
                        count() AS commit_count, \
                        math::sum(additions) AS total_additions, \
                        math::sum(deletions) AS total_deletions \
                 FROM touches \
                 WHERE out.project_id = $pid \
                 GROUP BY out.path",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to compute churn scores")?;

        let records: Vec<ChurnRecord> = resp.take(0)?;

        // Get co_changed counts per file (both directions).
        let mut co_change_counts: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();

        let mut resp2 = self
            .db
            .query(
                "SELECT in.path AS path, count() AS count \
                 FROM co_changed \
                 WHERE project_id = $pid \
                 GROUP BY in.path",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to count co-changes (in)")?;
        let in_counts: Vec<CoChangerRecord> = resp2.take(0)?;

        let mut resp3 = self
            .db
            .query(
                "SELECT out.path AS path, count() AS count \
                 FROM co_changed \
                 WHERE project_id = $pid \
                 GROUP BY out.path",
            )
            .bind(("pid", pid))
            .await
            .context("Failed to count co-changes (out)")?;
        let out_counts: Vec<CoChangerRecord> = resp3.take(0)?;

        for r in in_counts.into_iter().chain(out_counts) {
            if let Some(path) = r.path {
                *co_change_counts.entry(path).or_insert(0) += r.count.unwrap_or(0);
            }
        }

        // Compute scores.
        let mut scores: Vec<FileChurnScore> = Vec::new();
        let mut max_churn = 0i64;

        for r in &records {
            let total = r.total_additions.unwrap_or(0) + r.total_deletions.unwrap_or(0);
            if total > max_churn {
                max_churn = total;
            }
        }

        for r in records {
            let path = match r.path {
                Some(p) => p,
                None => continue,
            };
            let commit_count = r.commit_count.unwrap_or(0);
            let total_churn = r.total_additions.unwrap_or(0) + r.total_deletions.unwrap_or(0);
            let co_change_count = co_change_counts.get(&path).copied().unwrap_or(0);

            // Normalize churn score to 0.0-1.0 based on max_churn in project.
            let churn_score = if max_churn > 0 {
                (total_churn as f64 / max_churn as f64).min(1.0)
            } else {
                0.0
            };

            scores.push(FileChurnScore {
                path,
                commit_count,
                total_churn,
                co_change_count,
                churn_score,
            });
        }

        scores.sort_by(|a, b| {
            b.churn_score
                .partial_cmp(&a.churn_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(scores)
    }

    /// Batch-write churn scores to File nodes.
    pub async fn batch_update_churn_scores(&self, updates: &[FileChurnScore]) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        for chunk in updates.chunks(100) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPDATE $rid_{i} SET churn_score = $cs_{i} RETURN NONE;\n"
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
                    .bind((format!("cs_{i}"), update.churn_score));
            }
            q.await.context("Failed to batch update churn scores")?;
        }
        Ok(())
    }

    /// Get top N files by churn_score (pre-computed on File nodes).
    pub async fn get_top_hotspots(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> Result<Vec<FileChurnScore>> {
        let mut resp = self
            .db
            .query(
                "SELECT path, churn_score \
                 FROM `file` \
                 WHERE project_id = $pid AND churn_score IS NOT NONE \
                 ORDER BY churn_score DESC \
                 LIMIT $lim",
            )
            .bind(("pid", project_id.to_string()))
            .bind(("lim", limit as i64))
            .await
            .context("Failed to get top hotspots")?;

        let records: Vec<HotspotRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                Some(FileChurnScore {
                    path: r.path?,
                    commit_count: 0,
                    total_churn: 0,
                    co_change_count: 0,
                    churn_score: r.churn_score.unwrap_or(0.0),
                })
            })
            .collect())
    }

    // ========================================================================
    // Knowledge Density (3 methods)
    // ========================================================================

    /// Compute knowledge density per file based on linked notes and decisions.
    ///
    /// For each file in the project, count attached_to edges from note and
    /// decision nodes, then normalize to 0.0-1.0.
    pub async fn compute_knowledge_density(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<FileKnowledgeDensity>> {
        let pid = project_id.to_string();

        // Count notes attached to each file (via attached_to: note -> file).
        let mut resp_notes = self
            .db
            .query(
                "SELECT out.path AS path, count() AS count \
                 FROM attached_to \
                 WHERE out.project_id = $pid AND in.id IS NOT NONE \
                 AND record::tb(in) = 'note' \
                 GROUP BY out.path",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to count notes for density")?;

        let note_counts: Vec<CoChangerRecord> = resp_notes.take(0)?;

        // Count decisions attached to each file (via attached_to: decision -> file).
        let mut resp_decisions = self
            .db
            .query(
                "SELECT out.path AS path, count() AS count \
                 FROM attached_to \
                 WHERE out.project_id = $pid AND in.id IS NOT NONE \
                 AND record::tb(in) = 'decision' \
                 GROUP BY out.path",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to count decisions for density")?;

        let decision_counts: Vec<CoChangerRecord> = resp_decisions.take(0)?;

        // Get all project files.
        let mut resp_files = self
            .db
            .query("SELECT path FROM `file` WHERE project_id = $pid")
            .bind(("pid", pid))
            .await
            .context("Failed to list files for density")?;

        #[derive(Debug, SurrealValue)]
        struct PathOnly {
            path: Option<String>,
        }

        let file_paths: Vec<PathOnly> = resp_files.take(0)?;

        // Build maps.
        let mut note_map: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for r in note_counts {
            if let Some(path) = r.path {
                *note_map.entry(path).or_insert(0) += r.count.unwrap_or(0);
            }
        }

        let mut decision_map: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for r in decision_counts {
            if let Some(path) = r.path {
                *decision_map.entry(path).or_insert(0) += r.count.unwrap_or(0);
            }
        }

        // Compute density per file.
        let mut results: Vec<FileKnowledgeDensity> = Vec::new();
        let mut max_total = 0i64;

        // First pass: find max total knowledge items.
        for fp in &file_paths {
            if let Some(ref path) = fp.path {
                let nc = note_map.get(path).copied().unwrap_or(0);
                let dc = decision_map.get(path).copied().unwrap_or(0);
                let total = nc + dc;
                if total > max_total {
                    max_total = total;
                }
            }
        }

        // Second pass: compute normalized density.
        for fp in file_paths {
            if let Some(path) = fp.path {
                let note_count = note_map.get(&path).copied().unwrap_or(0);
                let decision_count = decision_map.get(&path).copied().unwrap_or(0);
                let total = note_count + decision_count;
                let density = if max_total > 0 {
                    total as f64 / max_total as f64
                } else {
                    0.0
                };

                results.push(FileKnowledgeDensity {
                    path,
                    note_count,
                    decision_count,
                    knowledge_density: density,
                });
            }
        }

        results.sort_by(|a, b| {
            a.knowledge_density
                .partial_cmp(&b.knowledge_density)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    /// Batch-write knowledge density scores to File nodes.
    pub async fn batch_update_knowledge_density(
        &self,
        updates: &[FileKnowledgeDensity],
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        for chunk in updates.chunks(100) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPDATE $rid_{i} SET knowledge_density = $kd_{i} RETURN NONE;\n"
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
                    .bind((format!("kd_{i}"), update.knowledge_density));
            }
            q.await
                .context("Failed to batch update knowledge density")?;
        }
        Ok(())
    }

    /// Get top N files with lowest knowledge_density (knowledge gaps).
    pub async fn get_top_knowledge_gaps(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> Result<Vec<FileKnowledgeDensity>> {
        let mut resp = self
            .db
            .query(
                "SELECT path, knowledge_density \
                 FROM `file` \
                 WHERE project_id = $pid AND knowledge_density IS NOT NONE \
                 ORDER BY knowledge_density ASC \
                 LIMIT $lim",
            )
            .bind(("pid", project_id.to_string()))
            .bind(("lim", limit as i64))
            .await
            .context("Failed to get top knowledge gaps")?;

        #[derive(Debug, SurrealValue)]
        struct GapRecord {
            path: Option<String>,
            knowledge_density: Option<f64>,
        }

        let records: Vec<GapRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                Some(FileKnowledgeDensity {
                    path: r.path?,
                    note_count: 0,
                    decision_count: 0,
                    knowledge_density: r.knowledge_density.unwrap_or(0.0),
                })
            })
            .collect())
    }

    // ========================================================================
    // Risk (3 methods)
    // ========================================================================

    /// Compute composite risk scores for all files in a project.
    ///
    /// Risk = weighted combination of:
    /// - PageRank (structural importance)
    /// - Churn score (change frequency)
    /// - Knowledge gap (1 - knowledge_density)
    /// - Betweenness centrality (bridge/bottleneck role)
    pub async fn compute_risk_scores(&self, project_id: Uuid) -> Result<Vec<FileRiskScore>> {
        let mut resp = self
            .db
            .query(
                "SELECT path, pagerank, betweenness, churn_score, knowledge_density \
                 FROM `file` \
                 WHERE project_id = $pid",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to query file analytics for risk")?;

        let records: Vec<FileAnalyticsRecord> = resp.take(0)?;

        let mut results: Vec<FileRiskScore> = Vec::new();
        for r in records {
            let path = match r.path {
                Some(p) => p,
                None => continue,
            };

            let pr = r.pagerank.unwrap_or(0.0);
            let bt = r.betweenness.unwrap_or(0.0);
            let churn = r.churn_score.unwrap_or(0.0);
            let density = r.knowledge_density.unwrap_or(0.0);
            let knowledge_gap = 1.0 - density;

            // Weighted risk: high churn + low knowledge + high structural importance = risky.
            let risk_score =
                (0.30 * churn + 0.25 * knowledge_gap + 0.25 * pr + 0.20 * bt).clamp(0.0, 1.0);

            let risk_level = if risk_score >= 0.75 {
                "critical"
            } else if risk_score >= 0.50 {
                "high"
            } else if risk_score >= 0.25 {
                "medium"
            } else {
                "low"
            }
            .to_string();

            results.push(FileRiskScore {
                path,
                risk_score,
                risk_level,
                factors: RiskFactors {
                    pagerank: pr,
                    churn,
                    knowledge_gap,
                    betweenness: bt,
                },
            });
        }

        results.sort_by(|a, b| {
            b.risk_score
                .partial_cmp(&a.risk_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    /// Batch-write composite risk scores to File nodes.
    pub async fn batch_update_risk_scores(&self, updates: &[FileRiskScore]) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        for chunk in updates.chunks(100) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPDATE $rid_{i} SET \
                     risk_score = $rs_{i}, \
                     risk_level = $rl_{i}, \
                     risk_factors = $rf_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, update) in chunk.iter().enumerate() {
                let factors_json = serde_json::json!({
                    "pagerank": update.factors.pagerank,
                    "churn": update.factors.churn,
                    "knowledge_gap": update.factors.knowledge_gap,
                    "betweenness": update.factors.betweenness,
                });
                q = q
                    .bind((
                        format!("rid_{i}"),
                        RecordId::new("file", update.path.as_str()),
                    ))
                    .bind((format!("rs_{i}"), update.risk_score))
                    .bind((format!("rl_{i}"), update.risk_level.clone()))
                    .bind((format!("rf_{i}"), factors_json));
            }
            q.await.context("Failed to batch update risk scores")?;
        }
        Ok(())
    }

    /// Get risk assessment summary stats for a project.
    pub async fn get_risk_summary(&self, project_id: Uuid) -> Result<serde_json::Value> {
        let pid = project_id.to_string();

        // Count files per risk level.
        let mut resp = self
            .db
            .query(
                "SELECT risk_level, count() AS count \
                 FROM `file` \
                 WHERE project_id = $pid AND risk_level IS NOT NONE \
                 GROUP BY risk_level",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to get risk summary")?;

        let levels: Vec<RiskLevelRecord> = resp.take(0)?;

        let mut summary = serde_json::json!({
            "critical": 0,
            "high": 0,
            "medium": 0,
            "low": 0,
            "total_assessed": 0,
        });

        let mut total = 0i64;
        for level in levels {
            if let Some(rl) = level.risk_level {
                summary[&rl] = serde_json::json!(level.count);
                total += level.count;
            }
        }
        summary["total_assessed"] = serde_json::json!(total);

        // Get total project files.
        let mut resp2 = self
            .db
            .query("SELECT count() AS count FROM `file` WHERE project_id = $pid GROUP ALL")
            .bind(("pid", pid))
            .await
            .context("Failed to count project files for risk summary")?;

        let counts: Vec<CountRecord> = resp2.take(0)?;
        let total_files = counts.into_iter().next().map(|r| r.count).unwrap_or(0);
        summary["total_files"] = serde_json::json!(total_files);

        Ok(summary)
    }

    // ========================================================================
    // Neural Metrics (1 method)
    // ========================================================================

    /// Get neural network metrics for a project's SYNAPSE layer.
    pub async fn get_neural_metrics(&self, project_id: Uuid) -> Result<NeuralMetrics> {
        let pid = project_id.to_string();

        // Count total synapses for this project's notes.
        let mut resp_synapses = self
            .db
            .query(
                "SELECT count() AS count \
                 FROM synapse \
                 WHERE in.project_id = $pid \
                 GROUP ALL",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to count synapses")?;

        let syn_counts: Vec<CountRecord> = resp_synapses.take(0)?;
        let active_synapses = syn_counts.into_iter().next().map(|r| r.count).unwrap_or(0);

        // Average energy of notes.
        let mut resp_energy = self
            .db
            .query(
                "SELECT math::mean(energy) AS avg_val \
                 FROM note \
                 WHERE project_id = $pid AND energy IS NOT NONE \
                 GROUP ALL",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to compute avg energy")?;

        let avg_records: Vec<AvgRecord> = resp_energy.take(0)?;
        let avg_energy = avg_records
            .into_iter()
            .next()
            .and_then(|r| r.avg_val)
            .unwrap_or(0.0);

        // Weak synapses ratio (weight < 0.3).
        let weak_count = if active_synapses > 0 {
            let mut resp_weak = self
                .db
                .query(
                    "SELECT count() AS count \
                     FROM synapse \
                     WHERE in.project_id = $pid AND weight < 0.3 \
                     GROUP ALL",
                )
                .bind(("pid", pid.clone()))
                .await
                .context("Failed to count weak synapses")?;

            let wc: Vec<CountRecord> = resp_weak.take(0)?;
            wc.into_iter().next().map(|r| r.count).unwrap_or(0)
        } else {
            0
        };

        let weak_synapses_ratio = if active_synapses > 0 {
            weak_count as f64 / active_synapses as f64
        } else {
            0.0
        };

        // Dead notes: notes with no synapse connections and low energy.
        let mut resp_dead = self
            .db
            .query(
                "SELECT count() AS count \
                 FROM note \
                 WHERE project_id = $pid \
                   AND (energy IS NONE OR energy < 0.1) \
                   AND id NOT IN (SELECT VALUE in FROM synapse) \
                   AND id NOT IN (SELECT VALUE out FROM synapse) \
                 GROUP ALL",
            )
            .bind(("pid", pid))
            .await
            .context("Failed to count dead notes")?;

        let dead_counts: Vec<CountRecord> = resp_dead.take(0)?;
        let dead_notes_count = dead_counts.into_iter().next().map(|r| r.count).unwrap_or(0);

        Ok(NeuralMetrics {
            active_synapses,
            avg_energy,
            weak_synapses_ratio,
            dead_notes_count,
        })
    }

    // ========================================================================
    // File Analytics Batch (2 methods)
    // ========================================================================

    /// Batch update fabric file analytics (pagerank, betweenness, community, clustering).
    pub async fn batch_update_fabric_file_analytics(
        &self,
        updates: &[FabricFileAnalyticsUpdate],
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        for chunk in updates.chunks(100) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPDATE $rid_{i} SET \
                     fabric_pagerank = $fpr_{i}, \
                     fabric_betweenness = $fbt_{i}, \
                     fabric_community_id = $fcid_{i}, \
                     fabric_community_label = $fcl_{i}, \
                     fabric_clustering_coefficient = $fcc_{i} \
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
                    .bind((format!("fpr_{i}"), update.fabric_pagerank))
                    .bind((format!("fbt_{i}"), update.fabric_betweenness))
                    .bind((format!("fcid_{i}"), update.fabric_community_id as i64))
                    .bind((format!("fcl_{i}"), update.fabric_community_label.clone()))
                    .bind((format!("fcc_{i}"), update.fabric_clustering_coefficient));
            }
            q.await
                .context("Failed to batch update fabric file analytics")?;
        }
        Ok(())
    }

    /// Batch update structural DNA vectors on File nodes.
    pub async fn batch_update_structural_dna(&self, updates: &[StructuralDnaUpdate]) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        for chunk in updates.chunks(100) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPDATE $rid_{i} SET structural_dna = $dna_{i} RETURN NONE;\n"
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
                    .bind((format!("dna_{i}"), update.dna.clone()));
            }
            q.await.context("Failed to batch update structural DNA")?;
        }
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::models::FileChangedInfo;
    use cortex_core::test_helpers::{test_commit, test_file_for_project, test_project};

    async fn setup() -> IndentiaGraphStore {
        IndentiaGraphStore::new_memory().await.unwrap()
    }

    /// Helper: create a project with files, commits, and touches for testing.
    async fn setup_with_data() -> (IndentiaGraphStore, Uuid) {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        // Create files.
        let f1 = test_file_for_project("/src/main.rs", project.id);
        let f2 = test_file_for_project("/src/lib.rs", project.id);
        let f3 = test_file_for_project("/src/utils.rs", project.id);
        store.upsert_file(&f1).await.unwrap();
        store.upsert_file(&f2).await.unwrap();
        store.upsert_file(&f3).await.unwrap();

        // Create commits that touch files.
        let c1 = test_commit("commit_1");
        let c2 = test_commit("commit_2");
        let c3 = test_commit("commit_3");
        store.create_commit(&c1).await.unwrap();
        store.create_commit(&c2).await.unwrap();
        store.create_commit(&c3).await.unwrap();

        // Commit 1 touches main.rs and lib.rs.
        store
            .create_commit_touches(
                "commit_1",
                &[
                    FileChangedInfo {
                        path: "/src/main.rs".to_string(),
                        additions: Some(10),
                        deletions: Some(2),
                    },
                    FileChangedInfo {
                        path: "/src/lib.rs".to_string(),
                        additions: Some(5),
                        deletions: Some(1),
                    },
                ],
            )
            .await
            .unwrap();

        // Commit 2 touches main.rs and lib.rs (again) and utils.rs.
        store
            .create_commit_touches(
                "commit_2",
                &[
                    FileChangedInfo {
                        path: "/src/main.rs".to_string(),
                        additions: Some(20),
                        deletions: Some(5),
                    },
                    FileChangedInfo {
                        path: "/src/lib.rs".to_string(),
                        additions: Some(3),
                        deletions: Some(0),
                    },
                    FileChangedInfo {
                        path: "/src/utils.rs".to_string(),
                        additions: Some(8),
                        deletions: Some(2),
                    },
                ],
            )
            .await
            .unwrap();

        // Commit 3 touches only utils.rs.
        store
            .create_commit_touches(
                "commit_3",
                &[FileChangedInfo {
                    path: "/src/utils.rs".to_string(),
                    additions: Some(15),
                    deletions: Some(3),
                }],
            )
            .await
            .unwrap();

        (store, project.id)
    }

    // ========================================================================
    // Test 1: Co-changed computation
    // ========================================================================

    #[tokio::test]
    async fn test_co_changed_computation() {
        let (store, project_id) = setup_with_data().await;

        let count = store
            .compute_co_changed(project_id, None, 1, 100)
            .await
            .unwrap();

        // Commit 1: main.rs + lib.rs => 1 pair
        // Commit 2: main.rs + lib.rs + utils.rs => 3 pairs
        // Expected pairs: (main, lib)=2, (main, utils)=1, (lib, utils)=1
        // All have count >= 1, so all should be created.
        assert!(
            count >= 3,
            "Expected at least 3 co-change relations, got {}",
            count
        );

        // Verify timestamp update works.
        store
            .update_project_co_change_timestamp(project_id)
            .await
            .unwrap();
        let project = store.get_project(project_id).await.unwrap().unwrap();
        assert!(project.last_co_change_computed_at.is_some());
    }

    // ========================================================================
    // Test 2: Co-change graph retrieval
    // ========================================================================

    #[tokio::test]
    async fn test_get_co_change_graph() {
        let (store, project_id) = setup_with_data().await;

        // First compute co-changes.
        store
            .compute_co_changed(project_id, None, 1, 100)
            .await
            .unwrap();

        // Get graph.
        let graph = store.get_co_change_graph(project_id, 1, 100).await.unwrap();

        assert!(!graph.is_empty(), "Co-change graph should not be empty");

        // The pair (main.rs, lib.rs) should have count=2.
        let main_lib = graph.iter().find(|p| {
            (p.file_a.contains("main") && p.file_b.contains("lib"))
                || (p.file_a.contains("lib") && p.file_b.contains("main"))
        });
        assert!(
            main_lib.is_some(),
            "Should find main.rs <-> lib.rs co-change pair"
        );
        assert_eq!(main_lib.unwrap().count, 2);
    }

    // ========================================================================
    // Test 3: Churn scores computation
    // ========================================================================

    #[tokio::test]
    async fn test_churn_scores() {
        let (store, project_id) = setup_with_data().await;

        let scores = store.compute_churn_scores(project_id).await.unwrap();
        assert!(!scores.is_empty(), "Should have churn scores");

        // main.rs: 2 commits, 10+2+20+5 = 37 churn.
        let main_score = scores.iter().find(|s| s.path == "/src/main.rs");
        assert!(main_score.is_some());
        let main_score = main_score.unwrap();
        assert_eq!(main_score.commit_count, 2);
        assert_eq!(main_score.total_churn, 37);

        // utils.rs: 2 commits, 8+2+15+3 = 28 churn.
        let utils_score = scores.iter().find(|s| s.path == "/src/utils.rs");
        assert!(utils_score.is_some());
        assert_eq!(utils_score.unwrap().commit_count, 2);

        // Scores should be normalized 0-1.
        for score in &scores {
            assert!(
                score.churn_score >= 0.0 && score.churn_score <= 1.0,
                "Churn score should be 0-1, got {}",
                score.churn_score
            );
        }

        // The file with highest churn should have score 1.0.
        assert!(
            (scores[0].churn_score - 1.0).abs() < f64::EPSILON,
            "Top churn file should have score 1.0"
        );
    }

    // ========================================================================
    // Test 4: Batch update churn scores
    // ========================================================================

    #[tokio::test]
    async fn test_batch_update_churn() {
        let (store, project_id) = setup_with_data().await;

        let updates = vec![
            FileChurnScore {
                path: "/src/main.rs".to_string(),
                commit_count: 5,
                total_churn: 100,
                co_change_count: 3,
                churn_score: 0.85,
            },
            FileChurnScore {
                path: "/src/lib.rs".to_string(),
                commit_count: 2,
                total_churn: 30,
                co_change_count: 1,
                churn_score: 0.35,
            },
        ];

        store.batch_update_churn_scores(&updates).await.unwrap();

        // Verify via hotspots.
        let hotspots = store.get_top_hotspots(project_id, 10).await.unwrap();
        assert!(!hotspots.is_empty());
        let main = hotspots.iter().find(|h| h.path == "/src/main.rs");
        assert!(main.is_some());
        assert!((main.unwrap().churn_score - 0.85).abs() < f64::EPSILON);
    }

    // ========================================================================
    // Test 5: Knowledge density computation
    // ========================================================================

    #[tokio::test]
    async fn test_knowledge_density() {
        let (store, project_id) = setup_with_data().await;

        // Create a note attached to main.rs.
        let note_id = Uuid::new_v4();
        let note_rid = RecordId::new("note", note_id.to_string().as_str());
        store
            .db
            .query(
                "CREATE $rid SET \
                 note_type = 'guideline', status = 'active', importance = 'high', \
                 content = 'Test note', created_at = $now, project_id = $pid \
                 RETURN NONE",
            )
            .bind(("rid", note_rid.clone()))
            .bind(("now", Utc::now().to_rfc3339()))
            .bind(("pid", project_id.to_string()))
            .await
            .unwrap();

        // Attach note to main.rs file.
        let file_rid = RecordId::new("file", "/src/main.rs");
        store
            .db
            .query("RELATE $from->attached_to->$to RETURN NONE")
            .bind(("from", note_rid))
            .bind(("to", file_rid))
            .await
            .unwrap();

        let density = store.compute_knowledge_density(project_id).await.unwrap();

        assert!(!density.is_empty());

        // main.rs should have note_count=1.
        let main_density = density.iter().find(|d| d.path == "/src/main.rs");
        assert!(main_density.is_some());
        assert_eq!(main_density.unwrap().note_count, 1);

        // lib.rs should have note_count=0.
        let lib_density = density.iter().find(|d| d.path == "/src/lib.rs");
        assert!(lib_density.is_some());
        assert_eq!(lib_density.unwrap().note_count, 0);
    }

    // ========================================================================
    // Test 6: Risk scores computation
    // ========================================================================

    #[tokio::test]
    async fn test_risk_scores() {
        let (store, project_id) = setup_with_data().await;

        // Set some analytics values on files.
        store
            .db
            .query(
                "UPDATE type::record('file', '/src/main.rs') SET \
                 pagerank = 0.8, betweenness = 0.6, \
                 churn_score = 0.9, knowledge_density = 0.1 \
                 RETURN NONE;\
                 UPDATE type::record('file', '/src/lib.rs') SET \
                 pagerank = 0.2, betweenness = 0.1, \
                 churn_score = 0.3, knowledge_density = 0.8 \
                 RETURN NONE",
            )
            .await
            .unwrap();

        let scores = store.compute_risk_scores(project_id).await.unwrap();
        assert!(!scores.is_empty());

        // main.rs should be higher risk (high churn, low knowledge).
        let main_risk = scores.iter().find(|s| s.path == "/src/main.rs").unwrap();
        let lib_risk = scores.iter().find(|s| s.path == "/src/lib.rs").unwrap();

        assert!(
            main_risk.risk_score > lib_risk.risk_score,
            "main.rs ({}) should have higher risk than lib.rs ({})",
            main_risk.risk_score,
            lib_risk.risk_score
        );

        // Check risk levels are valid.
        for score in &scores {
            assert!(
                ["low", "medium", "high", "critical"].contains(&score.risk_level.as_str()),
                "Invalid risk level: {}",
                score.risk_level
            );
        }

        // Check factors are populated.
        assert!(main_risk.factors.pagerank > 0.0);
        assert!(main_risk.factors.churn > 0.0);
    }

    // ========================================================================
    // Test 7: Neural metrics
    // ========================================================================

    #[tokio::test]
    async fn test_neural_metrics() {
        let (store, project_id) = setup_with_data().await;

        // Create some notes with energy.
        for i in 0..3 {
            let nid = Uuid::new_v4();
            let rid = RecordId::new("note", nid.to_string().as_str());
            store
                .db
                .query(
                    "CREATE $rid SET \
                     note_type = 'guideline', status = 'active', importance = 'medium', \
                     content = $content, energy = $energy, \
                     created_at = $now, project_id = $pid \
                     RETURN NONE",
                )
                .bind(("rid", rid))
                .bind(("content", format!("Note {}", i)))
                .bind(("energy", 0.5 + (i as f64) * 0.2))
                .bind(("now", Utc::now().to_rfc3339()))
                .bind(("pid", project_id.to_string()))
                .await
                .unwrap();
        }

        let metrics = store.get_neural_metrics(project_id).await.unwrap();

        // No synapses created, so active_synapses should be 0.
        assert_eq!(metrics.active_synapses, 0);
        // avg_energy should be (0.5 + 0.7 + 0.9) / 3 = 0.7.
        assert!(
            (metrics.avg_energy - 0.7).abs() < 0.01,
            "Expected avg_energy ~0.7, got {}",
            metrics.avg_energy
        );
        assert!(
            (metrics.weak_synapses_ratio - 0.0).abs() < f64::EPSILON,
            "No synapses means ratio should be 0"
        );
    }

    // ========================================================================
    // Test 8: Batch fabric analytics update
    // ========================================================================

    #[tokio::test]
    async fn test_batch_fabric_analytics() {
        let (store, _project_id) = setup_with_data().await;

        // Test fabric file analytics update.
        let updates = vec![
            FabricFileAnalyticsUpdate {
                path: "/src/main.rs".to_string(),
                fabric_pagerank: 0.95,
                fabric_betweenness: 0.42,
                fabric_community_id: 1,
                fabric_community_label: "core".to_string(),
                fabric_clustering_coefficient: 0.65,
            },
            FabricFileAnalyticsUpdate {
                path: "/src/lib.rs".to_string(),
                fabric_pagerank: 0.78,
                fabric_betweenness: 0.31,
                fabric_community_id: 1,
                fabric_community_label: "core".to_string(),
                fabric_clustering_coefficient: 0.55,
            },
        ];

        store
            .batch_update_fabric_file_analytics(&updates)
            .await
            .unwrap();

        // Verify by querying fabric fields.
        #[derive(Debug, SurrealValue)]
        struct FabricCheck {
            fabric_pagerank: Option<f64>,
            fabric_community_label: Option<String>,
        }

        let mut resp = store
            .db
            .query("SELECT fabric_pagerank, fabric_community_label FROM type::record('file', '/src/main.rs')")
            .await
            .unwrap();

        let records: Vec<FabricCheck> = resp.take(0).unwrap();
        assert!(!records.is_empty());
        let record = &records[0];
        assert!((record.fabric_pagerank.unwrap_or(0.0) - 0.95).abs() < f64::EPSILON);
        assert_eq!(record.fabric_community_label.as_deref(), Some("core"));

        // Test structural DNA update.
        let dna_updates = vec![
            StructuralDnaUpdate {
                path: "/src/main.rs".to_string(),
                dna: vec![0.1, 0.2, 0.3, 0.4, 0.5],
            },
            StructuralDnaUpdate {
                path: "/src/lib.rs".to_string(),
                dna: vec![0.5, 0.4, 0.3, 0.2, 0.1],
            },
        ];

        store
            .batch_update_structural_dna(&dna_updates)
            .await
            .unwrap();

        // Verify DNA was stored.
        #[derive(Debug, SurrealValue)]
        struct DnaCheck {
            structural_dna: Option<Vec<f64>>,
        }

        let mut resp2 = store
            .db
            .query("SELECT structural_dna FROM type::record('file', '/src/main.rs')")
            .await
            .unwrap();

        let dna_records: Vec<DnaCheck> = resp2.take(0).unwrap();
        assert!(!dna_records.is_empty());
        let dna = dna_records[0].structural_dna.as_ref().unwrap();
        assert_eq!(dna.len(), 5);
        assert!((dna[0] - 0.1).abs() < f64::EPSILON);
    }

    // ========================================================================
    // Test: Empty batch operations are no-ops
    // ========================================================================

    #[tokio::test]
    async fn test_empty_batch_operations() {
        let store = setup().await;

        store.batch_update_churn_scores(&[]).await.unwrap();
        store.batch_update_knowledge_density(&[]).await.unwrap();
        store.batch_update_risk_scores(&[]).await.unwrap();
        store.batch_update_fabric_file_analytics(&[]).await.unwrap();
        store.batch_update_structural_dna(&[]).await.unwrap();
    }

    // ========================================================================
    // Test: Risk summary
    // ========================================================================

    #[tokio::test]
    async fn test_risk_summary() {
        let (store, project_id) = setup_with_data().await;

        // Set risk levels on files.
        store
            .db
            .query(
                "UPDATE type::record('file', '/src/main.rs') SET \
                 risk_level = 'critical', risk_score = 0.9 RETURN NONE;\
                 UPDATE type::record('file', '/src/lib.rs') SET \
                 risk_level = 'low', risk_score = 0.1 RETURN NONE;\
                 UPDATE type::record('file', '/src/utils.rs') SET \
                 risk_level = 'medium', risk_score = 0.4 RETURN NONE",
            )
            .await
            .unwrap();

        let summary = store.get_risk_summary(project_id).await.unwrap();

        assert_eq!(summary["critical"], 1);
        assert_eq!(summary["low"], 1);
        assert_eq!(summary["medium"], 1);
        assert_eq!(summary["total_assessed"], 3);
        assert_eq!(summary["total_files"], 3);
    }

    // ========================================================================
    // Test: File co-changers
    // ========================================================================

    #[tokio::test]
    async fn test_file_co_changers() {
        let (store, project_id) = setup_with_data().await;

        store
            .compute_co_changed(project_id, None, 1, 100)
            .await
            .unwrap();

        let co_changers = store
            .get_file_co_changers("/src/main.rs", 1, 10)
            .await
            .unwrap();

        // main.rs co-changes with lib.rs (2 times) and utils.rs (1 time).
        assert!(!co_changers.is_empty(), "main.rs should have co-changers");

        let lib_changer = co_changers.iter().find(|c| c.path.contains("lib"));
        assert!(lib_changer.is_some(), "lib.rs should be a co-changer");
        assert_eq!(lib_changer.unwrap().count, 2);
    }
}
