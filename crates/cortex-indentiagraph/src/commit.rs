//! Commit CRUD operations for IndentiaGraphStore.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{CommitFileInfo, CommitNode, FileChangedInfo, FileHistoryEntry};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::IndentiaGraphStore;

#[derive(Debug, SurrealValue)]
struct CommitRecord {
    #[allow(dead_code)]
    id: RecordId,
    hash: String,
    message: String,
    author: String,
    timestamp: String,
}

#[derive(Debug, SurrealValue)]
struct TouchRecord {
    path: Option<String>,
    additions: Option<i64>,
    deletions: Option<i64>,
}

#[derive(Debug, SurrealValue)]
struct HistoryRecord {
    hash: Option<String>,
    message: Option<String>,
    author: Option<String>,
    timestamp: Option<String>,
    additions: Option<i64>,
    deletions: Option<i64>,
}

impl CommitRecord {
    fn into_node(self) -> CommitNode {
        CommitNode {
            hash: self.hash,
            message: self.message,
            author: self.author,
            timestamp: self
                .timestamp
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
        }
    }
}

impl IndentiaGraphStore {
    pub async fn create_commit(&self, commit: &CommitNode) -> Result<()> {
        let rid = RecordId::new("commit", commit.hash.as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 hash = $hash, message = $msg, author = $author, \
                 timestamp = $ts \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("hash", commit.hash.clone()))
            .bind(("msg", commit.message.clone()))
            .bind(("author", commit.author.clone()))
            .bind(("ts", commit.timestamp.to_rfc3339()))
            .await
            .context("Failed to create commit")?;
        Ok(())
    }

    pub async fn get_commit(&self, hash: &str) -> Result<Option<CommitNode>> {
        let rid = RecordId::new("commit", hash);
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get commit")?;
        let records: Vec<CommitRecord> = resp.take(0)?;
        Ok(records.into_iter().next().map(|r| r.into_node()))
    }

    pub async fn delete_commit(&self, hash: &str) -> Result<()> {
        let rid = RecordId::new("commit", hash);
        self.db
            .query(
                "DELETE FROM resolved_by WHERE out = type::record('commit', $h);\
                 DELETE FROM touches WHERE in = type::record('commit', $h);\
                 DELETE $rid",
            )
            .bind(("h", hash.to_string()))
            .bind(("rid", rid))
            .await
            .context("Failed to delete commit")?;
        Ok(())
    }

    pub async fn link_commit_to_task(&self, commit_hash: &str, task_id: Uuid) -> Result<()> {
        let task_rid = RecordId::new("task", task_id.to_string().as_str());
        let commit_rid = RecordId::new("commit", commit_hash);
        self.db
            .query("RELATE $from->resolved_by->$to RETURN NONE")
            .bind(("from", task_rid))
            .bind(("to", commit_rid))
            .await
            .context("Failed to link commit to task")?;
        Ok(())
    }

    pub async fn link_commit_to_plan(&self, commit_hash: &str, plan_id: Uuid) -> Result<()> {
        let plan_rid = RecordId::new("plan", plan_id.to_string().as_str());
        let commit_rid = RecordId::new("commit", commit_hash);
        self.db
            .query("RELATE $from->resulted_in->$to RETURN NONE")
            .bind(("from", plan_rid))
            .bind(("to", commit_rid))
            .await
            .context("Failed to link commit to plan")?;
        Ok(())
    }

    pub async fn get_task_commits(&self, task_id: Uuid) -> Result<Vec<CommitNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM commit WHERE hash IN \
                 (SELECT VALUE out.hash FROM resolved_by WHERE in = type::record('task', $tid))",
            )
            .bind(("tid", task_id.to_string()))
            .await
            .context("Failed to get task commits")?;
        let records: Vec<CommitRecord> = resp.take(0)?;
        Ok(records.into_iter().map(|r| r.into_node()).collect())
    }

    pub async fn get_plan_commits(&self, plan_id: Uuid) -> Result<Vec<CommitNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM commit WHERE hash IN \
                 (SELECT VALUE out.hash FROM resulted_in WHERE in = type::record('plan', $pid))",
            )
            .bind(("pid", plan_id.to_string()))
            .await
            .context("Failed to get plan commits")?;
        let records: Vec<CommitRecord> = resp.take(0)?;
        Ok(records.into_iter().map(|r| r.into_node()).collect())
    }

    pub async fn create_commit_touches(
        &self,
        commit_hash: &str,
        files: &[FileChangedInfo],
    ) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        for chunk in files.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "RELATE $from_{i}->touches->$to_{i} \
                     SET additions = $add_{i}, deletions = $del_{i} RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, file) in chunk.iter().enumerate() {
                q = q
                    .bind((format!("from_{i}"), RecordId::new("commit", commit_hash)))
                    .bind((format!("to_{i}"), RecordId::new("file", file.path.as_str())))
                    .bind((format!("add_{i}"), file.additions.unwrap_or(0)))
                    .bind((format!("del_{i}"), file.deletions.unwrap_or(0)));
            }
            q.await.context("Failed to create commit touches")?;
        }
        Ok(())
    }

    pub async fn get_commit_files(&self, commit_hash: &str) -> Result<Vec<CommitFileInfo>> {
        let mut resp = self
            .db
            .query(
                "SELECT out.path AS path, additions, deletions \
                 FROM touches WHERE in = type::record('commit', $h)",
            )
            .bind(("h", commit_hash.to_string()))
            .await
            .context("Failed to get commit files")?;

        let records: Vec<TouchRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                r.path.map(|p| CommitFileInfo {
                    path: p,
                    additions: r.additions,
                    deletions: r.deletions,
                })
            })
            .collect())
    }

    pub async fn get_file_history(
        &self,
        file_path: &str,
        limit: Option<i64>,
    ) -> Result<Vec<FileHistoryEntry>> {
        let lim = limit.unwrap_or(50);
        let mut resp = self
            .db
            .query(
                "SELECT in.hash AS hash, in.message AS message, in.author AS author, \
                 in.timestamp AS timestamp, additions, deletions \
                 FROM touches WHERE out = type::record('file', $fp) \
                 ORDER BY timestamp DESC LIMIT $lim",
            )
            .bind(("fp", file_path.to_string()))
            .bind(("lim", lim))
            .await
            .context("Failed to get file history")?;

        let records: Vec<HistoryRecord> = resp.take(0)?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                Some(FileHistoryEntry {
                    hash: r.hash?,
                    message: r.message.unwrap_or_default(),
                    author: r.author.unwrap_or_default(),
                    timestamp: r
                        .timestamp
                        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                        .unwrap_or_else(Utc::now),
                    additions: r.additions,
                    deletions: r.deletions,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::test_helpers::{test_commit, test_file, test_plan, test_task};

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_create_and_get_commit() {
        let store = setup().await;
        let commit = test_commit("abc123");
        store.create_commit(&commit).await.unwrap();

        let retrieved = store.get_commit("abc123").await.unwrap().unwrap();
        assert_eq!(retrieved.hash, "abc123");
        assert_eq!(retrieved.author, "test-author");
    }

    #[tokio::test]
    async fn test_get_nonexistent_commit() {
        let store = setup().await;
        assert!(store.get_commit("nonexistent").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_link_commit_to_task() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Task with commit");
        store.create_task(plan.id, &task).await.unwrap();
        let commit = test_commit("def456");
        store.create_commit(&commit).await.unwrap();

        store.link_commit_to_task("def456", task.id).await.unwrap();
        let commits = store.get_task_commits(task.id).await.unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "def456");
    }

    #[tokio::test]
    async fn test_link_commit_to_plan() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let commit = test_commit("ghi789");
        store.create_commit(&commit).await.unwrap();

        store.link_commit_to_plan("ghi789", plan.id).await.unwrap();
        let commits = store.get_plan_commits(plan.id).await.unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "ghi789");
    }

    #[tokio::test]
    async fn test_commit_touches_files() {
        let store = setup().await;
        let f1 = test_file("/src/main.rs");
        let f2 = test_file("/src/lib.rs");
        store.upsert_file(&f1).await.unwrap();
        store.upsert_file(&f2).await.unwrap();

        let commit = test_commit("touch123");
        store.create_commit(&commit).await.unwrap();

        let files = vec![
            FileChangedInfo {
                path: "/src/main.rs".to_string(),
                additions: Some(10),
                deletions: Some(3),
            },
            FileChangedInfo {
                path: "/src/lib.rs".to_string(),
                additions: Some(5),
                deletions: None,
            },
        ];
        store
            .create_commit_touches("touch123", &files)
            .await
            .unwrap();

        let touched = store.get_commit_files("touch123").await.unwrap();
        assert_eq!(touched.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_commit() {
        let store = setup().await;
        let commit = test_commit("del789");
        store.create_commit(&commit).await.unwrap();

        store.delete_commit("del789").await.unwrap();
        assert!(store.get_commit("del789").await.unwrap().is_none());
    }
}
