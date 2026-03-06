//! Task CRUD and dependency operations for IndentiaGraphStore.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{PlanNode, ProjectNode, TaskNode, TaskStatus, TaskWithPlan};
use cortex_core::plan::UpdateTaskRequest;
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

#[derive(Debug, SurrealValue)]
struct TaskRecord {
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
    #[allow(dead_code)]
    plan_id: Option<String>,
}

impl TaskRecord {
    fn into_node(self) -> Result<TaskNode> {
        Ok(TaskNode {
            id: rid_to_uuid(&self.id)?,
            title: self.title,
            description: self.description,
            status: parse_task_status(&self.status),
            priority: self.priority.map(|p| p as i32),
            assigned_to: self.assigned_to,
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            updated_at: self.updated_at.and_then(|s| s.parse().ok()),
            started_at: self.started_at.and_then(|s| s.parse().ok()),
            completed_at: self.completed_at.and_then(|s| s.parse().ok()),
            tags: json_str_to_vec(&self.tags),
            acceptance_criteria: json_str_to_vec(&self.acceptance_criteria),
            affected_files: json_str_to_vec(&self.affected_files),
            estimated_complexity: self.estimated_complexity.map(|v| v as u32),
            actual_complexity: self.actual_complexity.map(|v| v as u32),
        })
    }
}

pub(crate) fn json_str_to_vec(s: &Option<String>) -> Vec<String> {
    s.as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

fn vec_to_json_str(v: &[String]) -> Option<String> {
    if v.is_empty() {
        None
    } else {
        Some(serde_json::to_string(v).unwrap_or_default())
    }
}

pub(crate) fn parse_task_status(s: &str) -> TaskStatus {
    match s {
        "pending" => TaskStatus::Pending,
        "in_progress" => TaskStatus::InProgress,
        "blocked" => TaskStatus::Blocked,
        "completed" => TaskStatus::Completed,
        "failed" => TaskStatus::Failed,
        _ => TaskStatus::Pending,
    }
}

fn status_to_str(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
    }
}

impl IndentiaGraphStore {
    pub async fn create_task(&self, plan_id: Uuid, task: &TaskNode) -> Result<()> {
        let rid = RecordId::new("task", task.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 title = $title, description = $desc, \
                 status = $status, priority = $priority, assigned_to = $assigned_to, \
                 created_at = $created_at, tags = $tags, \
                 acceptance_criteria = $ac, affected_files = $af, \
                 estimated_complexity = $ec, actual_complexity = $axc, \
                 plan_id = $plan_id \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("title", task.title.clone()))
            .bind(("desc", task.description.clone()))
            .bind(("status", status_to_str(&task.status).to_string()))
            .bind(("priority", task.priority.map(|p| p as i64)))
            .bind(("assigned_to", task.assigned_to.clone()))
            .bind(("created_at", task.created_at.to_rfc3339()))
            .bind(("tags", vec_to_json_str(&task.tags)))
            .bind(("ac", vec_to_json_str(&task.acceptance_criteria)))
            .bind(("af", vec_to_json_str(&task.affected_files)))
            .bind(("ec", task.estimated_complexity.map(|v| v as i64)))
            .bind(("axc", task.actual_complexity.map(|v| v as i64)))
            .bind(("plan_id", plan_id.to_string()))
            .await
            .context("Failed to create task")?;

        // Create HAS_TASK edge
        let plan_rid = RecordId::new("plan", plan_id.to_string().as_str());
        let task_rid = RecordId::new("task", task.id.to_string().as_str());
        self.db
            .query("RELATE $from->has_task->$to RETURN NONE")
            .bind(("from", plan_rid))
            .bind(("to", task_rid))
            .await
            .context("Failed to create HAS_TASK edge")?;

        Ok(())
    }

    pub async fn get_task(&self, task_id: Uuid) -> Result<Option<TaskNode>> {
        let rid = RecordId::new("task", task_id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get task")?;
        let records: Vec<TaskRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn get_plan_tasks(&self, plan_id: Uuid) -> Result<Vec<TaskNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM task WHERE plan_id = $pid ORDER BY priority DESC")
            .bind(("pid", plan_id.to_string()))
            .await
            .context("Failed to get plan tasks")?;
        let records: Vec<TaskRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn update_task_status(&self, task_id: Uuid, status: TaskStatus) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let started = if status == TaskStatus::InProgress {
            Some(now.clone())
        } else {
            None
        };
        let completed = if status == TaskStatus::Completed {
            Some(now.clone())
        } else {
            None
        };
        let rid = RecordId::new("task", task_id.to_string().as_str());
        self.db
            .query(
                "UPDATE $rid SET status = $status, updated_at = $now, \
                 started_at = IF $started != NONE THEN $started ELSE started_at END, \
                 completed_at = IF $completed != NONE THEN $completed ELSE completed_at END \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("status", status_to_str(&status).to_string()))
            .bind(("now", now))
            .bind(("started", started))
            .bind(("completed", completed))
            .await
            .context("Failed to update task status")?;
        Ok(())
    }

    pub async fn update_task(&self, task_id: Uuid, updates: &UpdateTaskRequest) -> Result<()> {
        let mut sets = vec!["updated_at = $now".to_string()];
        if updates.title.is_some() {
            sets.push("title = $title".to_string());
        }
        if updates.description.is_some() {
            sets.push("description = $desc".to_string());
        }
        if updates.status.is_some() {
            sets.push("status = $status".to_string());
        }
        if updates.assigned_to.is_some() {
            sets.push("assigned_to = $assigned_to".to_string());
        }
        if updates.priority.is_some() {
            sets.push("priority = $priority".to_string());
        }
        if updates.tags.is_some() {
            sets.push("tags = $tags".to_string());
        }
        if updates.acceptance_criteria.is_some() {
            sets.push("acceptance_criteria = $ac".to_string());
        }
        if updates.affected_files.is_some() {
            sets.push("affected_files = $af".to_string());
        }
        if updates.actual_complexity.is_some() {
            sets.push("actual_complexity = $axc".to_string());
        }

        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let rid = RecordId::new("task", task_id.to_string().as_str());
        let mut q = self.db.query(&query);
        q = q.bind(("rid", rid)).bind(("now", Utc::now().to_rfc3339()));

        if let Some(ref t) = updates.title {
            q = q.bind(("title", t.clone()));
        }
        if let Some(ref d) = updates.description {
            q = q.bind(("desc", d.clone()));
        }
        if let Some(ref s) = updates.status {
            q = q.bind(("status", status_to_str(s).to_string()));
        }
        if let Some(ref a) = updates.assigned_to {
            q = q.bind(("assigned_to", a.clone()));
        }
        if let Some(p) = updates.priority {
            q = q.bind(("priority", p as i64));
        }
        if let Some(ref tags) = updates.tags {
            q = q.bind(("tags", vec_to_json_str(tags)));
        }
        if let Some(ref ac) = updates.acceptance_criteria {
            q = q.bind(("ac", vec_to_json_str(ac)));
        }
        if let Some(ref af) = updates.affected_files {
            q = q.bind(("af", vec_to_json_str(af)));
        }
        if let Some(axc) = updates.actual_complexity {
            q = q.bind(("axc", axc as i64));
        }

        q.await.context("Failed to update task")?;
        Ok(())
    }

    pub async fn assign_task(&self, task_id: Uuid, agent_id: &str) -> Result<()> {
        let rid = RecordId::new("task", task_id.to_string().as_str());
        self.db
            .query("UPDATE $rid SET assigned_to = $agent, updated_at = $now RETURN NONE")
            .bind(("rid", rid))
            .bind(("agent", agent_id.to_string()))
            .bind(("now", Utc::now().to_rfc3339()))
            .await
            .context("Failed to assign task")?;
        Ok(())
    }

    pub async fn delete_task(&self, task_id: Uuid) -> Result<()> {
        let tid = task_id.to_string();
        let rid = RecordId::new("task", tid.as_str());
        self.db
            .query(
                "DELETE FROM has_task WHERE out = type::record('task', $tid);\
                 DELETE FROM depends_on WHERE in = type::record('task', $tid) OR out = type::record('task', $tid);\
                 DELETE FROM has_step WHERE in = type::record('task', $tid);\
                 DELETE step WHERE task_id = $tid;\
                 DELETE $rid",
            )
            .bind(("tid", tid))
            .bind(("rid", rid))
            .await
            .context("Failed to delete task")?;
        Ok(())
    }

    pub async fn add_task_dependency(&self, task_id: Uuid, depends_on_id: Uuid) -> Result<()> {
        let from = RecordId::new("task", task_id.to_string().as_str());
        let to = RecordId::new("task", depends_on_id.to_string().as_str());
        self.db
            .query("RELATE $from->depends_on->$to RETURN NONE")
            .bind(("from", from))
            .bind(("to", to))
            .await
            .context("Failed to add task dependency")?;
        Ok(())
    }

    pub async fn remove_task_dependency(&self, task_id: Uuid, depends_on_id: Uuid) -> Result<()> {
        let tid = task_id.to_string();
        let did = depends_on_id.to_string();
        self.db
            .query("DELETE FROM depends_on WHERE in = type::record('task', $tid) AND out = type::record('task', $did)")
            .bind(("tid", tid))
            .bind(("did", did))
            .await
            .context("Failed to remove task dependency")?;
        Ok(())
    }

    pub async fn get_task_dependencies(&self, task_id: Uuid) -> Result<Vec<TaskNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM task WHERE id IN \
                 (SELECT VALUE out.id FROM depends_on WHERE in = type::record('task', $tid))",
            )
            .bind(("tid", task_id.to_string()))
            .await
            .context("Failed to get task dependencies")?;
        let records: Vec<TaskRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn get_task_blockers(&self, task_id: Uuid) -> Result<Vec<TaskNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM task WHERE id IN \
                 (SELECT VALUE out.id FROM depends_on WHERE in = type::record('task', $tid)) \
                 AND status NOT IN ['completed']",
            )
            .bind(("tid", task_id.to_string()))
            .await
            .context("Failed to get task blockers")?;
        let records: Vec<TaskRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn get_tasks_blocked_by(&self, task_id: Uuid) -> Result<Vec<TaskNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM task WHERE id IN \
                 (SELECT VALUE in.id FROM depends_on WHERE out = type::record('task', $tid)) \
                 AND status NOT IN ['completed']",
            )
            .bind(("tid", task_id.to_string()))
            .await
            .context("Failed to get tasks blocked by")?;
        let records: Vec<TaskRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn get_plan_dependency_graph(
        &self,
        plan_id: Uuid,
    ) -> Result<(Vec<TaskNode>, Vec<(Uuid, Uuid)>)> {
        let tasks = self.get_plan_tasks(plan_id).await?;
        let task_ids: Vec<String> = tasks.iter().map(|t| t.id.to_string()).collect();

        if task_ids.is_empty() {
            return Ok((tasks, vec![]));
        }

        let mut resp = self
            .db
            .query(
                "SELECT in.id AS from_id, out.id AS to_id FROM depends_on WHERE in.plan_id = $pid",
            )
            .bind(("pid", plan_id.to_string()))
            .await
            .context("Failed to get dependency graph")?;
        let edges: Vec<serde_json::Value> = resp.take(0)?;
        let mut deps = Vec::new();
        for edge in edges {
            if let (Some(from), Some(to)) = (
                edge.get("from_id").and_then(|v| v.as_str()),
                edge.get("to_id").and_then(|v| v.as_str()),
            ) {
                if let (Ok(f), Ok(t)) = (Uuid::parse_str(from), Uuid::parse_str(to)) {
                    deps.push((f, t));
                }
            }
        }

        Ok((tasks, deps))
    }

    pub async fn get_plan_critical_path(&self, plan_id: Uuid) -> Result<Vec<TaskNode>> {
        // Get all tasks and dependencies, then compute longest path
        let (tasks, deps) = self.get_plan_dependency_graph(plan_id).await?;
        if tasks.is_empty() {
            return Ok(vec![]);
        }

        use std::collections::{HashMap, HashSet};
        let task_map: HashMap<Uuid, &TaskNode> = tasks.iter().map(|t| (t.id, t)).collect();
        let mut adj: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        let mut in_degree: HashMap<Uuid, usize> = HashMap::new();

        for t in &tasks {
            adj.entry(t.id).or_default();
            in_degree.entry(t.id).or_insert(0);
        }
        for (from, to) in &deps {
            adj.entry(*from).or_default().push(*to);
            *in_degree.entry(*to).or_insert(0) += 1;
        }

        // Topological sort + longest path
        let mut queue: Vec<Uuid> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut dist: HashMap<Uuid, usize> = queue.iter().map(|id| (*id, 1)).collect();
        let mut parent: HashMap<Uuid, Option<Uuid>> = queue.iter().map(|id| (*id, None)).collect();
        let mut visited = HashSet::new();

        while let Some(node) = queue.pop() {
            if !visited.insert(node) {
                continue;
            }
            let d = dist[&node];
            for &next in adj.get(&node).unwrap_or(&vec![]) {
                if d + 1 > *dist.get(&next).unwrap_or(&0) {
                    dist.insert(next, d + 1);
                    parent.insert(next, Some(node));
                }
                let deg = in_degree.get_mut(&next).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push(next);
                }
            }
        }

        // Find the task with longest distance
        let end = dist.iter().max_by_key(|(_, &d)| d).map(|(id, _)| *id);
        let mut path = Vec::new();
        let mut current = end;
        while let Some(id) = current {
            if let Some(task) = task_map.get(&id) {
                path.push((*task).clone());
            }
            current = parent.get(&id).copied().flatten();
        }
        path.reverse();
        Ok(path)
    }

    pub async fn get_next_available_task(&self, plan_id: Uuid) -> Result<Option<TaskNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM task WHERE plan_id = $pid AND status = 'pending' \
                 AND assigned_to = NONE \
                 ORDER BY priority DESC LIMIT 1",
            )
            .bind(("pid", plan_id.to_string()))
            .await
            .context("Failed to get next available task")?;
        let records: Vec<TaskRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn get_project_for_task(&self, task_id: Uuid) -> Result<Option<ProjectNode>> {
        // Task -> plan_id -> plan.project_id -> project
        let rid = RecordId::new("task", task_id.to_string().as_str());
        let mut resp = self
            .db
            .query(
                "LET $t = (SELECT plan_id FROM $rid);\
                 LET $pid = (SELECT project_id FROM plan WHERE id = $t[0].plan_id LIMIT 1);\
                 SELECT * FROM project WHERE id = $pid[0].project_id LIMIT 1",
            )
            .bind(("rid", rid))
            .await
            .context("Failed to get project for task")?;

        let records: Vec<serde_json::Value> = resp.take(2)?;
        if let Some(record) = records.first() {
            let id_str = record.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if let Ok(id) = Uuid::parse_str(id_str) {
                return self.get_project(id).await;
            }
        }
        Ok(None)
    }

    pub async fn get_project_tasks(&self, project_id: Uuid) -> Result<Vec<TaskNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM task WHERE plan_id IN \
                 (SELECT VALUE id FROM plan WHERE project_id = $pid) \
                 ORDER BY priority DESC",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to get project tasks")?;
        let records: Vec<TaskRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn get_project_progress(&self, project_id: Uuid) -> Result<(u32, u32, u32, u32)> {
        let tasks = self.get_project_tasks(project_id).await?;
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

    pub async fn get_project_task_dependencies(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<(Uuid, Uuid)>> {
        let mut resp = self
            .db
            .query(
                "SELECT in.id AS from_id, out.id AS to_id FROM depends_on \
                 WHERE in.plan_id IN (SELECT VALUE id FROM plan WHERE project_id = $pid)",
            )
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to get project task dependencies")?;
        let edges: Vec<serde_json::Value> = resp.take(0)?;
        let mut deps = Vec::new();
        for edge in edges {
            if let (Some(from), Some(to)) = (
                edge.get("from_id").and_then(|v| v.as_str()),
                edge.get("to_id").and_then(|v| v.as_str()),
            ) {
                if let (Ok(f), Ok(t)) = (Uuid::parse_str(from), Uuid::parse_str(to)) {
                    deps.push((f, t));
                }
            }
        }
        Ok(deps)
    }

    pub async fn list_all_tasks_filtered(
        &self,
        plan_id: Option<Uuid>,
        project_id: Option<Uuid>,
        _workspace_slug: Option<&str>,
        statuses: Option<Vec<String>>,
        priority_min: Option<i32>,
        priority_max: Option<i32>,
        tags: Option<Vec<String>>,
        assigned_to: Option<&str>,
        limit: usize,
        offset: usize,
        sort_by: Option<&str>,
        sort_order: &str,
    ) -> Result<(Vec<TaskWithPlan>, usize)> {
        let mut conditions = Vec::new();

        if let Some(pid) = plan_id {
            conditions.push(format!("plan_id = '{}'", pid));
        }
        if let Some(proj_id) = project_id {
            conditions.push(format!(
                "plan_id IN (SELECT VALUE id FROM plan WHERE project_id = '{}')",
                proj_id
            ));
        }
        if let Some(ref statuses) = statuses {
            let list: Vec<String> = statuses.iter().map(|s| format!("'{}'", s)).collect();
            conditions.push(format!("status IN [{}]", list.join(",")));
        }
        if let Some(min) = priority_min {
            conditions.push(format!("priority >= {}", min));
        }
        if let Some(max) = priority_max {
            conditions.push(format!("priority <= {}", max));
        }
        if let Some(ref _tags) = tags {
            // Tags are JSON-encoded, complex filter — skip for now
        }
        if let Some(agent) = assigned_to {
            conditions.push(format!("assigned_to = '{}'", agent));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let order_field = sort_by.unwrap_or("created_at");
        let order_dir = if sort_order == "asc" { "ASC" } else { "DESC" };

        let count_q = format!(
            "SELECT count() AS total FROM task {} GROUP ALL",
            where_clause
        );
        let data_q = format!(
            "SELECT * FROM task {} ORDER BY {} {} LIMIT {} START {}",
            where_clause, order_field, order_dir, limit, offset
        );

        let mut resp = self
            .db
            .query(format!("{}; {}", count_q, data_q))
            .await
            .context("Failed to list tasks filtered")?;

        let count_result: Vec<serde_json::Value> = resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let records: Vec<TaskRecord> = resp.take(1)?;
        let mut tasks_with_plans = Vec::new();
        for r in records {
            let plan_id_str = r.plan_id.clone().unwrap_or_default();
            let task = r.into_node()?;
            // Lookup plan info
            let plan_id = Uuid::parse_str(&plan_id_str).unwrap_or_default();
            let plan = self.get_plan(plan_id).await?.unwrap_or_else(|| PlanNode {
                id: plan_id,
                title: "Unknown".to_string(),
                description: String::new(),
                status: cortex_core::models::PlanStatus::Draft,
                priority: 0,
                created_by: String::new(),
                created_at: Utc::now(),
                project_id: None,
            });
            tasks_with_plans.push(TaskWithPlan {
                task,
                plan_id,
                plan_title: plan.title,
                plan_status: Some(format!("{:?}", plan.status).to_lowercase()),
            });
        }

        Ok((tasks_with_plans, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::test_helpers::{test_plan, test_task};

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_create_and_get_task() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        let task = test_task("Implement feature X");
        store.create_task(plan.id, &task).await.unwrap();

        let retrieved = store.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, task.id);
        assert_eq!(retrieved.description, "Implement feature X");
        assert_eq!(retrieved.status, TaskStatus::Pending);
    }

    #[tokio::test]
    async fn test_get_plan_tasks() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        let t1 = test_task("Task 1");
        let t2 = test_task("Task 2");
        store.create_task(plan.id, &t1).await.unwrap();
        store.create_task(plan.id, &t2).await.unwrap();

        let tasks = store.get_plan_tasks(plan.id).await.unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_update_task_status() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Task to update");
        store.create_task(plan.id, &task).await.unwrap();

        store
            .update_task_status(task.id, TaskStatus::InProgress)
            .await
            .unwrap();
        let updated = store.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(updated.status, TaskStatus::InProgress);
        assert!(updated.started_at.is_some());
    }

    #[tokio::test]
    async fn test_update_task_partial() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Original description");
        store.create_task(plan.id, &task).await.unwrap();

        let updates = UpdateTaskRequest {
            title: Some("New Title".to_string()),
            priority: Some(9),
            ..Default::default()
        };
        store.update_task(task.id, &updates).await.unwrap();

        let updated = store.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(updated.title, Some("New Title".to_string()));
        assert_eq!(updated.priority, Some(9));
        assert_eq!(updated.description, "Original description");
    }

    #[tokio::test]
    async fn test_assign_task() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Assignable task");
        store.create_task(plan.id, &task).await.unwrap();

        store.assign_task(task.id, "agent-1").await.unwrap();
        let assigned = store.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(assigned.assigned_to, Some("agent-1".to_string()));
    }

    #[tokio::test]
    async fn test_delete_task() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Delete me");
        store.create_task(plan.id, &task).await.unwrap();

        store.delete_task(task.id).await.unwrap();
        assert!(store.get_task(task.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_task_dependencies() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        let t1 = test_task("Setup DB");
        let t2 = test_task("Implement API");
        let t3 = test_task("Write tests");
        store.create_task(plan.id, &t1).await.unwrap();
        store.create_task(plan.id, &t2).await.unwrap();
        store.create_task(plan.id, &t3).await.unwrap();

        // t2 depends on t1, t3 depends on t2
        store.add_task_dependency(t2.id, t1.id).await.unwrap();
        store.add_task_dependency(t3.id, t2.id).await.unwrap();

        let deps = store.get_task_dependencies(t2.id).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].id, t1.id);

        let blockers = store.get_task_blockers(t3.id).await.unwrap();
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].id, t2.id);
    }

    #[tokio::test]
    async fn test_remove_dependency() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        let t1 = test_task("Task A");
        let t2 = test_task("Task B");
        store.create_task(plan.id, &t1).await.unwrap();
        store.create_task(plan.id, &t2).await.unwrap();

        store.add_task_dependency(t2.id, t1.id).await.unwrap();
        assert_eq!(store.get_task_dependencies(t2.id).await.unwrap().len(), 1);

        store.remove_task_dependency(t2.id, t1.id).await.unwrap();
        assert_eq!(store.get_task_dependencies(t2.id).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_get_next_available_task() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        let mut t1 = test_task("Already assigned");
        t1.assigned_to = Some("agent-1".to_string());
        let t2 = test_task("Available task");

        store.create_task(plan.id, &t1).await.unwrap();
        store.create_task(plan.id, &t2).await.unwrap();

        let next = store
            .get_next_available_task(plan.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.id, t2.id);
    }

    #[tokio::test]
    async fn test_task_with_tags_and_criteria() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        let mut task = test_task("Tagged task");
        task.tags = vec!["backend".to_string(), "api".to_string()];
        task.acceptance_criteria = vec!["Tests pass".to_string()];
        task.affected_files = vec!["src/main.rs".to_string()];
        store.create_task(plan.id, &task).await.unwrap();

        let retrieved = store.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(retrieved.tags, vec!["backend", "api"]);
        assert_eq!(retrieved.acceptance_criteria, vec!["Tests pass"]);
        assert_eq!(retrieved.affected_files, vec!["src/main.rs"]);
    }
}
