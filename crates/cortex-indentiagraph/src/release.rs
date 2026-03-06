//! Release CRUD operations for IndentiaGraphStore.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{CommitNode, ReleaseNode, ReleaseStatus, TaskNode};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

#[derive(Debug, SurrealValue)]
struct ReleaseRecord {
    id: RecordId,
    version: String,
    title: Option<String>,
    description: Option<String>,
    status: String,
    project_id: String,
    target_date: Option<String>,
    created_at: String,
    released_at: Option<String>,
}

impl ReleaseRecord {
    fn into_node(self) -> Result<ReleaseNode> {
        Ok(ReleaseNode {
            id: rid_to_uuid(&self.id)?,
            version: self.version,
            title: self.title,
            description: self.description,
            status: parse_release_status(&self.status),
            project_id: Uuid::parse_str(&self.project_id)?,
            target_date: self.target_date.and_then(|s| s.parse().ok()),
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            released_at: self.released_at.and_then(|s| s.parse().ok()),
        })
    }
}

fn parse_release_status(s: &str) -> ReleaseStatus {
    match s {
        "planned" => ReleaseStatus::Planned,
        "in_progress" => ReleaseStatus::InProgress,
        "released" => ReleaseStatus::Released,
        "cancelled" => ReleaseStatus::Cancelled,
        _ => ReleaseStatus::Planned,
    }
}

/// Inline task record for release task queries.
#[derive(Debug, SurrealValue)]
struct ReleaseTaskRecord {
    id: RecordId,
    title: Option<String>,
    description: String,
    status: String,
    priority: Option<i64>,
    assigned_to: Option<String>,
    created_at: String,
    updated_at: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
    tags: Option<String>,
    acceptance_criteria: Option<String>,
    affected_files: Option<String>,
    estimated_complexity: Option<i64>,
    actual_complexity: Option<i64>,
}

/// Inline commit record for release commit queries.
#[derive(Debug, SurrealValue)]
struct ReleaseCommitRecord {
    #[allow(dead_code)]
    id: RecordId,
    hash: String,
    message: String,
    author: String,
    timestamp: String,
}

fn status_to_str(s: &ReleaseStatus) -> &'static str {
    match s {
        ReleaseStatus::Planned => "planned",
        ReleaseStatus::InProgress => "in_progress",
        ReleaseStatus::Released => "released",
        ReleaseStatus::Cancelled => "cancelled",
    }
}

impl IndentiaGraphStore {
    pub async fn create_release(&self, release: &ReleaseNode) -> Result<()> {
        let rid = RecordId::new("release", release.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 version = $ver, title = $title, description = $desc, \
                 status = $status, project_id = $pid, target_date = $td, \
                 created_at = $created_at, released_at = $ra \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("ver", release.version.clone()))
            .bind(("title", release.title.clone()))
            .bind(("desc", release.description.clone()))
            .bind(("status", status_to_str(&release.status).to_string()))
            .bind(("pid", release.project_id.to_string()))
            .bind(("td", release.target_date.map(|d| d.to_rfc3339())))
            .bind(("created_at", release.created_at.to_rfc3339()))
            .bind(("ra", release.released_at.map(|d| d.to_rfc3339())))
            .await
            .context("Failed to create release")?;
        Ok(())
    }

    pub async fn get_release(&self, id: Uuid) -> Result<Option<ReleaseNode>> {
        let rid = RecordId::new("release", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get release")?;
        let records: Vec<ReleaseRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn list_project_releases(&self, project_id: Uuid) -> Result<Vec<ReleaseNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM release WHERE project_id = $pid ORDER BY created_at DESC")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to list project releases")?;
        let records: Vec<ReleaseRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn update_release(
        &self,
        id: Uuid,
        status: Option<ReleaseStatus>,
        target_date: Option<DateTime<Utc>>,
        released_at: Option<DateTime<Utc>>,
        title: Option<String>,
        description: Option<String>,
    ) -> Result<()> {
        let mut sets = Vec::new();
        if status.is_some() {
            sets.push("status = $status");
        }
        if target_date.is_some() {
            sets.push("target_date = $td");
        }
        if released_at.is_some() {
            sets.push("released_at = $ra");
        }
        if title.is_some() {
            sets.push("title = $title");
        }
        if description.is_some() {
            sets.push("description = $desc");
        }
        if sets.is_empty() {
            return Ok(());
        }

        let rid = RecordId::new("release", id.to_string().as_str());
        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let mut q = self.db.query(&query);
        q = q.bind(("rid", rid));
        if let Some(ref s) = status {
            q = q.bind(("status", status_to_str(s).to_string()));
        }
        if let Some(d) = target_date {
            q = q.bind(("td", d.to_rfc3339()));
        }
        if let Some(d) = released_at {
            q = q.bind(("ra", d.to_rfc3339()));
        }
        if let Some(ref t) = title {
            q = q.bind(("title", t.clone()));
        }
        if let Some(ref d) = description {
            q = q.bind(("desc", d.clone()));
        }

        q.await.context("Failed to update release")?;
        Ok(())
    }

    pub async fn delete_release(&self, release_id: Uuid) -> Result<()> {
        let rid_str = release_id.to_string();
        let del_rid = RecordId::new("release", rid_str.as_str());
        self.db
            .query(
                "DELETE FROM includes_task WHERE in = type::record('release', $rid);\
                 DELETE FROM includes_commit WHERE in = type::record('release', $rid);\
                 DELETE $del_rid",
            )
            .bind(("rid", rid_str))
            .bind(("del_rid", del_rid))
            .await
            .context("Failed to delete release")?;
        Ok(())
    }

    pub async fn add_task_to_release(&self, release_id: Uuid, task_id: Uuid) -> Result<()> {
        let rel_rid = RecordId::new("release", release_id.to_string().as_str());
        let task_rid = RecordId::new("task", task_id.to_string().as_str());
        self.db
            .query("RELATE $from->includes_task->$to RETURN NONE")
            .bind(("from", rel_rid))
            .bind(("to", task_rid))
            .await
            .context("Failed to add task to release")?;
        Ok(())
    }

    pub async fn add_commit_to_release(&self, release_id: Uuid, commit_hash: &str) -> Result<()> {
        let rel_rid = RecordId::new("release", release_id.to_string().as_str());
        let commit_rid = RecordId::new("commit", commit_hash);
        self.db
            .query("RELATE $from->includes_commit->$to RETURN NONE")
            .bind(("from", rel_rid))
            .bind(("to", commit_rid))
            .await
            .context("Failed to add commit to release")?;
        Ok(())
    }

    pub async fn remove_commit_from_release(
        &self,
        release_id: Uuid,
        commit_hash: &str,
    ) -> Result<()> {
        let rel_rid = RecordId::new("release", release_id.to_string().as_str());
        let commit_rid = RecordId::new("commit", commit_hash);
        self.db
            .query("DELETE FROM includes_commit WHERE in = $rel_rid AND out = $commit_rid")
            .bind(("rel_rid", rel_rid))
            .bind(("commit_rid", commit_rid))
            .await
            .context("Failed to remove commit from release")?;
        Ok(())
    }

    pub async fn get_release_tasks(&self, release_id: Uuid) -> Result<Vec<TaskNode>> {
        let tasks_raw = self.get_release_task_records(release_id).await?;
        Ok(tasks_raw)
    }

    async fn get_release_task_records(&self, release_id: Uuid) -> Result<Vec<TaskNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM task WHERE id IN \
                 (SELECT VALUE out.id FROM includes_task WHERE in = type::record('release', $rid))",
            )
            .bind(("rid", release_id.to_string()))
            .await
            .context("Failed to get release tasks")?;

        let records: Vec<ReleaseTaskRecord> = resp.take(0)?;
        records
            .into_iter()
            .map(|r| {
                Ok(TaskNode {
                    id: crate::client::rid_to_uuid(&r.id)?,
                    title: r.title,
                    description: r.description,
                    status: crate::task::parse_task_status(&r.status),
                    priority: r.priority.map(|p| p as i32),
                    assigned_to: r.assigned_to,
                    created_at: r.created_at.parse().unwrap_or_else(|_| Utc::now()),
                    updated_at: r.updated_at.and_then(|s| s.parse().ok()),
                    started_at: r.started_at.and_then(|s| s.parse().ok()),
                    completed_at: r.completed_at.and_then(|s| s.parse().ok()),
                    tags: crate::task::json_str_to_vec(&r.tags),
                    acceptance_criteria: crate::task::json_str_to_vec(&r.acceptance_criteria),
                    affected_files: crate::task::json_str_to_vec(&r.affected_files),
                    estimated_complexity: r.estimated_complexity.map(|v| v as u32),
                    actual_complexity: r.actual_complexity.map(|v| v as u32),
                })
            })
            .collect()
    }

    pub async fn get_release_details(
        &self,
        release_id: Uuid,
    ) -> Result<Option<(ReleaseNode, Vec<TaskNode>, Vec<CommitNode>)>> {
        let release = match self.get_release(release_id).await? {
            Some(r) => r,
            None => return Ok(None),
        };
        let tasks = self.get_release_tasks(release_id).await?;

        // Get commits
        let mut resp = self
            .db
            .query(
                "SELECT * FROM commit WHERE hash IN \
                 (SELECT VALUE out.hash FROM includes_commit WHERE in = type::record('release', $rid))",
            )
            .bind(("rid", release_id.to_string()))
            .await
            .context("Failed to get release commits")?;

        let commit_records: Vec<ReleaseCommitRecord> = resp.take(0)?;
        let commits: Vec<CommitNode> = commit_records
            .into_iter()
            .map(|r| CommitNode {
                hash: r.hash,
                message: r.message,
                author: r.author,
                timestamp: r.timestamp.parse().unwrap_or_else(|_| Utc::now()),
            })
            .collect();

        Ok(Some((release, tasks, commits)))
    }

    pub async fn list_releases_filtered(
        &self,
        project_id: Uuid,
        statuses: Option<Vec<String>>,
        limit: usize,
        offset: usize,
        sort_by: Option<&str>,
        sort_order: &str,
    ) -> Result<(Vec<ReleaseNode>, usize)> {
        let mut conditions: Vec<&'static str> = vec!["project_id = $filter_pid"];
        if statuses.is_some() {
            conditions.push("status IN $filter_statuses");
        }
        let where_clause = format!("WHERE {}", conditions.join(" AND "));

        // Allowlist for ORDER BY
        let order_field = match sort_by.unwrap_or("created_at") {
            "created_at" | "updated_at" | "version" | "status" | "target_date" => {
                sort_by.unwrap_or("created_at")
            }
            _ => "created_at",
        };
        let order_dir = if sort_order == "asc" { "ASC" } else { "DESC" };

        let count_q = format!("SELECT count() AS total FROM release {where_clause} GROUP ALL");
        let data_q = format!(
            "SELECT * FROM release {where_clause} \
             ORDER BY {order_field} {order_dir} LIMIT $limit START $offset"
        );

        let query_str = format!("{count_q}; {data_q}");
        let mut qb = self
            .db
            .query(&query_str)
            .bind(("filter_pid", project_id.to_string()))
            .bind(("limit", limit as i64))
            .bind(("offset", offset as i64));
        if let Some(ref s) = statuses {
            qb = qb.bind(("filter_statuses", s.clone()));
        }

        let mut resp = qb.await?;
        let count_result: Vec<serde_json::Value> = resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let records: Vec<ReleaseRecord> = resp.take(1)?;
        let releases = records
            .into_iter()
            .filter_map(|r| r.into_node().ok())
            .collect();
        Ok((releases, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::test_helpers::{
        test_commit, test_plan, test_project, test_release, test_task,
    };

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_create_and_get_release() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let release = test_release(project.id);
        store.create_release(&release).await.unwrap();

        let retrieved = store.get_release(release.id).await.unwrap().unwrap();
        assert_eq!(retrieved.version, "1.0.0");
        assert_eq!(retrieved.status, ReleaseStatus::Planned);
    }

    #[tokio::test]
    async fn test_list_project_releases() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let mut r1 = test_release(project.id);
        r1.version = "1.0.0".to_string();
        let mut r2 = test_release(project.id);
        r2.version = "2.0.0".to_string();
        store.create_release(&r1).await.unwrap();
        store.create_release(&r2).await.unwrap();

        let releases = store.list_project_releases(project.id).await.unwrap();
        assert_eq!(releases.len(), 2);
    }

    #[tokio::test]
    async fn test_update_release() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let release = test_release(project.id);
        store.create_release(&release).await.unwrap();

        store
            .update_release(
                release.id,
                Some(ReleaseStatus::Released),
                None,
                Some(Utc::now()),
                None,
                None,
            )
            .await
            .unwrap();
        let updated = store.get_release(release.id).await.unwrap().unwrap();
        assert_eq!(updated.status, ReleaseStatus::Released);
        assert!(updated.released_at.is_some());
    }

    #[tokio::test]
    async fn test_release_with_tasks_and_commits() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let release = test_release(project.id);
        store.create_release(&release).await.unwrap();

        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Release task");
        store.create_task(plan.id, &task).await.unwrap();
        let commit = test_commit("rel123");
        store.create_commit(&commit).await.unwrap();

        store
            .add_task_to_release(release.id, task.id)
            .await
            .unwrap();
        store
            .add_commit_to_release(release.id, "rel123")
            .await
            .unwrap();

        let (rel, tasks, commits) = store
            .get_release_details(release.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rel.version, "1.0.0");
        assert_eq!(tasks.len(), 1);
        assert_eq!(commits.len(), 1);
    }

    #[tokio::test]
    async fn test_delete_release() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let release = test_release(project.id);
        store.create_release(&release).await.unwrap();

        store.delete_release(release.id).await.unwrap();
        assert!(store.get_release(release.id).await.unwrap().is_none());
    }
}
