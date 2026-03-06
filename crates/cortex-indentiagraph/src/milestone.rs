//! Milestone CRUD operations for IndentiaGraphStore.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{
    MilestoneNode, MilestoneStatus, StepNode, TaskNode, TaskStatus, TaskWithPlan,
};
use std::collections::HashMap;
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

#[derive(Debug, SurrealValue)]
struct MilestoneRecord {
    id: RecordId,
    title: String,
    description: Option<String>,
    status: String,
    project_id: String,
    target_date: Option<String>,
    closed_at: Option<String>,
    created_at: String,
    #[allow(dead_code)]
    updated_at: Option<String>,
}

impl MilestoneRecord {
    fn into_node(self) -> Result<MilestoneNode> {
        Ok(MilestoneNode {
            id: rid_to_uuid(&self.id)?,
            title: self.title,
            description: self.description,
            status: parse_milestone_status(&self.status),
            project_id: Uuid::parse_str(&self.project_id)?,
            target_date: self.target_date.and_then(|s| s.parse().ok()),
            closed_at: self.closed_at.and_then(|s| s.parse().ok()),
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
        })
    }
}

fn parse_milestone_status(s: &str) -> MilestoneStatus {
    match s {
        "planned" => MilestoneStatus::Planned,
        "open" => MilestoneStatus::Open,
        "in_progress" => MilestoneStatus::InProgress,
        "completed" => MilestoneStatus::Completed,
        "closed" => MilestoneStatus::Closed,
        _ => MilestoneStatus::Open,
    }
}

/// Inline task record for milestone task queries.
#[derive(Debug, SurrealValue)]
struct MilestoneTaskRecord {
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

fn status_to_str(s: &MilestoneStatus) -> &'static str {
    match s {
        MilestoneStatus::Planned => "planned",
        MilestoneStatus::Open => "open",
        MilestoneStatus::InProgress => "in_progress",
        MilestoneStatus::Completed => "completed",
        MilestoneStatus::Closed => "closed",
    }
}

impl IndentiaGraphStore {
    pub async fn create_milestone(&self, milestone: &MilestoneNode) -> Result<()> {
        let rid = RecordId::new("milestone", milestone.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 title = $title, description = $desc, \
                 status = $status, project_id = $pid, target_date = $td, \
                 closed_at = $ca, created_at = $created_at \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("title", milestone.title.clone()))
            .bind(("desc", milestone.description.clone()))
            .bind(("status", status_to_str(&milestone.status).to_string()))
            .bind(("pid", milestone.project_id.to_string()))
            .bind(("td", milestone.target_date.map(|d| d.to_rfc3339())))
            .bind(("ca", milestone.closed_at.map(|d| d.to_rfc3339())))
            .bind(("created_at", milestone.created_at.to_rfc3339()))
            .await
            .context("Failed to create milestone")?;
        Ok(())
    }

    pub async fn get_milestone(&self, id: Uuid) -> Result<Option<MilestoneNode>> {
        let rid = RecordId::new("milestone", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get milestone")?;
        let records: Vec<MilestoneRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn list_project_milestones(&self, project_id: Uuid) -> Result<Vec<MilestoneNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM milestone WHERE project_id = $pid ORDER BY created_at DESC")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to list project milestones")?;
        let records: Vec<MilestoneRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn update_milestone(
        &self,
        id: Uuid,
        status: Option<MilestoneStatus>,
        target_date: Option<DateTime<Utc>>,
        closed_at: Option<DateTime<Utc>>,
        title: Option<String>,
        description: Option<String>,
    ) -> Result<()> {
        let mut sets = vec!["updated_at = $now".to_string()];
        if status.is_some() {
            sets.push("status = $status".to_string());
        }
        if target_date.is_some() {
            sets.push("target_date = $td".to_string());
        }
        if closed_at.is_some() {
            sets.push("closed_at = $ca".to_string());
        }
        if title.is_some() {
            sets.push("title = $title".to_string());
        }
        if description.is_some() {
            sets.push("description = $desc".to_string());
        }

        let rid = RecordId::new("milestone", id.to_string().as_str());
        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let mut q = self.db.query(&query);
        q = q.bind(("rid", rid)).bind(("now", Utc::now().to_rfc3339()));
        if let Some(ref s) = status {
            q = q.bind(("status", status_to_str(s).to_string()));
        }
        if let Some(d) = target_date {
            q = q.bind(("td", d.to_rfc3339()));
        }
        if let Some(d) = closed_at {
            q = q.bind(("ca", d.to_rfc3339()));
        }
        if let Some(ref t) = title {
            q = q.bind(("title", t.clone()));
        }
        if let Some(ref d) = description {
            q = q.bind(("desc", d.clone()));
        }

        q.await.context("Failed to update milestone")?;
        Ok(())
    }

    pub async fn delete_milestone(&self, milestone_id: Uuid) -> Result<()> {
        let mid = milestone_id.to_string();
        let rid = RecordId::new("milestone", mid.as_str());
        self.db
            .query(
                "DELETE FROM includes_task WHERE in = type::record('milestone', $mid);\
                 DELETE $rid",
            )
            .bind(("mid", mid))
            .bind(("rid", rid))
            .await
            .context("Failed to delete milestone")?;
        Ok(())
    }

    pub async fn add_task_to_milestone(&self, milestone_id: Uuid, task_id: Uuid) -> Result<()> {
        let ms_rid = RecordId::new("milestone", milestone_id.to_string().as_str());
        let task_rid = RecordId::new("task", task_id.to_string().as_str());
        self.db
            .query("RELATE $from->includes_task->$to RETURN NONE")
            .bind(("from", ms_rid))
            .bind(("to", task_rid))
            .await
            .context("Failed to add task to milestone")?;
        Ok(())
    }

    pub async fn get_milestone_tasks(&self, milestone_id: Uuid) -> Result<Vec<TaskNode>> {
        self.get_release_task_records_for("milestone", milestone_id)
            .await
    }

    async fn get_release_task_records_for(&self, table: &str, id: Uuid) -> Result<Vec<TaskNode>> {
        let query = format!(
            "SELECT * FROM task WHERE id IN \
             (SELECT VALUE out.id FROM includes_task WHERE in = type::record('{}', $rid))",
            table
        );
        let mut resp = self
            .db
            .query(&query)
            .bind(("rid", id.to_string()))
            .await
            .context("Failed to get tasks")?;

        let records: Vec<MilestoneTaskRecord> = resp.take(0)?;
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

    pub async fn get_milestone_details(
        &self,
        milestone_id: Uuid,
    ) -> Result<Option<(MilestoneNode, Vec<TaskNode>)>> {
        let milestone = match self.get_milestone(milestone_id).await? {
            Some(m) => m,
            None => return Ok(None),
        };
        let tasks = self.get_milestone_tasks(milestone_id).await?;
        Ok(Some((milestone, tasks)))
    }

    pub async fn get_milestone_progress(&self, milestone_id: Uuid) -> Result<(u32, u32, u32, u32)> {
        let tasks = self.get_milestone_tasks(milestone_id).await?;
        let total = tasks.len() as u32;
        let completed = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count() as u32;
        let in_progress = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::InProgress)
            .count() as u32;
        let blocked = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Blocked)
            .count() as u32;
        Ok((total, completed, in_progress, blocked))
    }

    pub async fn get_milestone_tasks_with_plans(
        &self,
        milestone_id: Uuid,
    ) -> Result<Vec<TaskWithPlan>> {
        let tasks = self.get_milestone_tasks(milestone_id).await?;
        let mut result = Vec::new();
        for task in tasks {
            // Look up plan
            let mut resp = self
                .db
                .query("SELECT id, title, status FROM plan WHERE id = (SELECT VALUE plan_id FROM task WHERE id = $tid LIMIT 1)[0] LIMIT 1")
                .bind(("tid", task.id.to_string()))
                .await?;
            let plan_info: Vec<serde_json::Value> = resp.take(0)?;
            let (plan_id, plan_title, plan_status) = if let Some(p) = plan_info.first() {
                (
                    p.get("id")
                        .and_then(|v| v.as_str())
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .unwrap_or_default(),
                    p.get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown")
                        .to_string(),
                    p.get("status")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                )
            } else {
                (Uuid::default(), "Unknown".to_string(), None)
            };
            result.push(TaskWithPlan {
                task,
                plan_id,
                plan_title,
                plan_status,
            });
        }
        Ok(result)
    }

    pub async fn get_milestone_steps_batch(
        &self,
        milestone_id: Uuid,
    ) -> Result<HashMap<Uuid, Vec<StepNode>>> {
        let tasks = self.get_milestone_tasks(milestone_id).await?;
        let mut result = HashMap::new();
        for task in &tasks {
            let steps = self.get_task_steps(task.id).await?;
            result.insert(task.id, steps);
        }
        Ok(result)
    }

    pub async fn list_milestones_filtered(
        &self,
        project_id: Uuid,
        statuses: Option<Vec<String>>,
        limit: usize,
        offset: usize,
        sort_by: Option<&str>,
        sort_order: &str,
    ) -> Result<(Vec<MilestoneNode>, usize)> {
        let mut conditions = vec![format!("project_id = '{}'", project_id)];
        if let Some(ref statuses) = statuses {
            let list: Vec<String> = statuses.iter().map(|s| format!("'{}'", s)).collect();
            conditions.push(format!("status IN [{}]", list.join(",")));
        }
        let where_clause = format!("WHERE {}", conditions.join(" AND "));
        let order_field = sort_by.unwrap_or("created_at");
        let order_dir = if sort_order == "asc" { "ASC" } else { "DESC" };

        let count_q = format!(
            "SELECT count() AS total FROM milestone {} GROUP ALL",
            where_clause
        );
        let data_q = format!(
            "SELECT * FROM milestone {} ORDER BY {} {} LIMIT {} START {}",
            where_clause, order_field, order_dir, limit, offset
        );

        let mut resp = self.db.query(format!("{}; {}", count_q, data_q)).await?;
        let count_result: Vec<serde_json::Value> = resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let records: Vec<MilestoneRecord> = resp.take(1)?;
        let milestones = records
            .into_iter()
            .filter_map(|r| r.into_node().ok())
            .collect();
        Ok((milestones, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::test_helpers::{
        test_milestone, test_plan, test_project, test_step, test_task,
    };

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_create_and_get_milestone() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let ms = test_milestone(project.id);
        store.create_milestone(&ms).await.unwrap();

        let retrieved = store.get_milestone(ms.id).await.unwrap().unwrap();
        assert_eq!(retrieved.title, "v1.0 Milestone");
        assert_eq!(retrieved.status, MilestoneStatus::Open);
    }

    #[tokio::test]
    async fn test_list_project_milestones() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let m1 = test_milestone(project.id);
        let mut m2 = test_milestone(project.id);
        m2.title = "v2.0 Milestone".to_string();
        store.create_milestone(&m1).await.unwrap();
        store.create_milestone(&m2).await.unwrap();

        let milestones = store.list_project_milestones(project.id).await.unwrap();
        assert_eq!(milestones.len(), 2);
    }

    #[tokio::test]
    async fn test_update_milestone() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let ms = test_milestone(project.id);
        store.create_milestone(&ms).await.unwrap();

        store
            .update_milestone(
                ms.id,
                Some(MilestoneStatus::Completed),
                None,
                Some(Utc::now()),
                None,
                None,
            )
            .await
            .unwrap();
        let updated = store.get_milestone(ms.id).await.unwrap().unwrap();
        assert_eq!(updated.status, MilestoneStatus::Completed);
        assert!(updated.closed_at.is_some());
    }

    #[tokio::test]
    async fn test_milestone_with_tasks() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let ms = test_milestone(project.id);
        store.create_milestone(&ms).await.unwrap();

        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let t1 = test_task("Task 1");
        let mut t2 = test_task("Task 2");
        t2.status = TaskStatus::Completed;
        store.create_task(plan.id, &t1).await.unwrap();
        store.create_task(plan.id, &t2).await.unwrap();

        store.add_task_to_milestone(ms.id, t1.id).await.unwrap();
        store.add_task_to_milestone(ms.id, t2.id).await.unwrap();

        let (total, completed, _, _) = store.get_milestone_progress(ms.id).await.unwrap();
        assert_eq!(total, 2);
        assert_eq!(completed, 1);
    }

    #[tokio::test]
    async fn test_milestone_steps_batch() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let ms = test_milestone(project.id);
        store.create_milestone(&ms).await.unwrap();

        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Task with steps");
        store.create_task(plan.id, &task).await.unwrap();
        store.add_task_to_milestone(ms.id, task.id).await.unwrap();

        let s1 = test_step(0, "Step A");
        let s2 = test_step(1, "Step B");
        store.create_step(task.id, &s1).await.unwrap();
        store.create_step(task.id, &s2).await.unwrap();

        let batch = store.get_milestone_steps_batch(ms.id).await.unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[&task.id].len(), 2);
    }

    #[tokio::test]
    async fn test_delete_milestone() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        let ms = test_milestone(project.id);
        store.create_milestone(&ms).await.unwrap();

        store.delete_milestone(ms.id).await.unwrap();
        assert!(store.get_milestone(ms.id).await.unwrap().is_none());
    }
}
