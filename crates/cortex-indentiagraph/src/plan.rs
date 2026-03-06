//! Plan CRUD operations for IndentiaGraphStore.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{PlanNode, PlanStatus};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

#[derive(Debug, SurrealValue)]
struct PlanRecord {
    id: RecordId,
    title: String,
    description: String,
    status: String,
    priority: i64,
    created_by: String,
    created_at: String,
    #[allow(dead_code)]
    updated_at: Option<String>,
    project_id: Option<String>,
}

impl PlanRecord {
    fn into_node(self) -> Result<PlanNode> {
        Ok(PlanNode {
            id: rid_to_uuid(&self.id)?,
            title: self.title,
            description: self.description,
            status: parse_plan_status(&self.status),
            priority: self.priority as i32,
            created_by: self.created_by,
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            project_id: self.project_id.and_then(|s| Uuid::parse_str(&s).ok()),
        })
    }
}

fn parse_plan_status(s: &str) -> PlanStatus {
    match s {
        "draft" => PlanStatus::Draft,
        "approved" => PlanStatus::Approved,
        "in_progress" => PlanStatus::InProgress,
        "completed" => PlanStatus::Completed,
        "cancelled" => PlanStatus::Cancelled,
        _ => PlanStatus::Draft,
    }
}

fn status_to_string(s: &PlanStatus) -> &'static str {
    match s {
        PlanStatus::Draft => "draft",
        PlanStatus::Approved => "approved",
        PlanStatus::InProgress => "in_progress",
        PlanStatus::Completed => "completed",
        PlanStatus::Cancelled => "cancelled",
    }
}

impl IndentiaGraphStore {
    pub async fn create_plan(&self, plan: &PlanNode) -> Result<()> {
        let rid = RecordId::new("plan", plan.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 title = $title, description = $desc, \
                 status = $status, priority = $priority, created_by = $created_by, \
                 created_at = $created_at, project_id = $project_id \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("title", plan.title.clone()))
            .bind(("desc", plan.description.clone()))
            .bind(("status", status_to_string(&plan.status).to_string()))
            .bind(("priority", plan.priority as i64))
            .bind(("created_by", plan.created_by.clone()))
            .bind(("created_at", plan.created_at.to_rfc3339()))
            .bind(("project_id", plan.project_id.map(|id| id.to_string())))
            .await
            .context("Failed to create plan")?;

        // If plan has a project, create HAS_PLAN edge
        if let Some(project_id) = plan.project_id {
            let proj_rid = RecordId::new("project", project_id.to_string().as_str());
            let plan_rid = RecordId::new("plan", plan.id.to_string().as_str());
            self.db
                .query("RELATE $from->has_plan->$to RETURN NONE")
                .bind(("from", proj_rid))
                .bind(("to", plan_rid))
                .await
                .context("Failed to create HAS_PLAN edge")?;
        }
        Ok(())
    }

    pub async fn get_plan(&self, id: Uuid) -> Result<Option<PlanNode>> {
        let rid = RecordId::new("plan", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get plan")?;
        let records: Vec<PlanRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn update_plan_status(&self, id: Uuid, status: PlanStatus) -> Result<()> {
        let rid = RecordId::new("plan", id.to_string().as_str());
        self.db
            .query("UPDATE $rid SET status = $status, updated_at = $now RETURN NONE")
            .bind(("rid", rid))
            .bind(("status", status_to_string(&status).to_string()))
            .bind(("now", Utc::now().to_rfc3339()))
            .await
            .context("Failed to update plan status")?;
        Ok(())
    }

    pub async fn delete_plan(&self, plan_id: Uuid) -> Result<()> {
        let rid = RecordId::new("plan", plan_id.to_string().as_str());
        self.db
            .query(
                "DELETE FROM has_plan WHERE out = $rid;\
                 DELETE $rid",
            )
            .bind(("rid", rid))
            .await
            .context("Failed to delete plan")?;
        Ok(())
    }

    pub async fn link_plan_to_project(&self, plan_id: Uuid, project_id: Uuid) -> Result<()> {
        let plan_id_str = plan_id.to_string();
        let proj_rid = RecordId::new("project", project_id.to_string().as_str());
        let plan_rid = RecordId::new("plan", plan_id_str.as_str());
        let plan_rid2 = RecordId::new("plan", plan_id_str.as_str());
        self.db
            .query(
                "UPDATE $rid SET project_id = $pid RETURN NONE;\
                 RELATE $from->has_plan->$to RETURN NONE",
            )
            .bind(("rid", plan_rid))
            .bind(("pid", project_id.to_string()))
            .bind(("from", proj_rid))
            .bind(("to", plan_rid2))
            .await
            .context("Failed to link plan to project")?;
        Ok(())
    }

    pub async fn unlink_plan_from_project(&self, plan_id: Uuid) -> Result<()> {
        let plan_id_str = plan_id.to_string();
        let rid = RecordId::new("plan", plan_id_str.as_str());
        let rid2 = RecordId::new("plan", plan_id_str.as_str());
        self.db
            .query(
                "UPDATE $rid SET project_id = NONE RETURN NONE;\
                 DELETE FROM has_plan WHERE out = $rid2",
            )
            .bind(("rid", rid))
            .bind(("rid2", rid2))
            .await
            .context("Failed to unlink plan from project")?;
        Ok(())
    }

    pub async fn list_active_plans(&self) -> Result<Vec<PlanNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM plan WHERE status IN ['draft', 'approved', 'in_progress'] ORDER BY priority DESC")
            .await
            .context("Failed to list active plans")?;
        let records: Vec<PlanRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn list_project_plans(&self, project_id: Uuid) -> Result<Vec<PlanNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM plan WHERE project_id = $pid ORDER BY priority DESC")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to list project plans")?;
        let records: Vec<PlanRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn count_project_plans(&self, project_id: Uuid) -> Result<i64> {
        let mut resp = self
            .db
            .query("SELECT count() AS total FROM plan WHERE project_id = $pid GROUP ALL")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to count project plans")?;
        let result: Vec<serde_json::Value> = resp.take(0)?;
        Ok(result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0))
    }

    pub async fn list_plans_for_project(
        &self,
        project_id: Uuid,
        status_filter: Option<Vec<String>>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<PlanNode>, usize)> {
        let (plans, total) = self
            .list_plans_filtered(
                Some(project_id),
                None,
                status_filter,
                None,
                None,
                None,
                limit,
                offset,
                None,
                "desc",
            )
            .await?;
        Ok((plans, total))
    }

    pub async fn list_plans_filtered(
        &self,
        project_id: Option<Uuid>,
        _workspace_slug: Option<&str>,
        statuses: Option<Vec<String>>,
        priority_min: Option<i32>,
        priority_max: Option<i32>,
        search: Option<&str>,
        limit: usize,
        offset: usize,
        sort_by: Option<&str>,
        sort_order: &str,
    ) -> Result<(Vec<PlanNode>, usize)> {
        let mut conditions: Vec<&'static str> = Vec::new();

        if project_id.is_some() {
            conditions.push("project_id = $filter_pid");
        }
        if statuses.is_some() {
            conditions.push("status IN $filter_statuses");
        }
        if priority_min.is_some() {
            conditions.push("priority >= $filter_priority_min");
        }
        if priority_max.is_some() {
            conditions.push("priority <= $filter_priority_max");
        }
        if search.is_some() {
            conditions.push(
                "(string::lowercase(title) CONTAINS $filter_search \
                 OR string::lowercase(description ?? '') CONTAINS $filter_search)",
            );
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Allowlist for ORDER BY — prevents injection via sort_by
        let order_field = match sort_by.unwrap_or("created_at") {
            "created_at" | "updated_at" | "title" | "priority" | "status" => {
                sort_by.unwrap_or("created_at")
            }
            _ => "created_at",
        };
        let order_dir = if sort_order == "asc" { "ASC" } else { "DESC" };

        let count_q = format!("SELECT count() AS total FROM plan {where_clause} GROUP ALL");
        let data_q = format!(
            "SELECT * FROM plan {where_clause} ORDER BY {order_field} {order_dir} LIMIT $limit START $offset"
        );

        let query_str = format!("{count_q}; {data_q}");
        let mut qb = self
            .db
            .query(&query_str)
            .bind(("limit", limit as i64))
            .bind(("offset", offset as i64));

        if let Some(pid) = project_id {
            qb = qb.bind(("filter_pid", pid.to_string()));
        }
        if let Some(ref s) = statuses {
            qb = qb.bind(("filter_statuses", s.clone()));
        }
        if let Some(min) = priority_min {
            qb = qb.bind(("filter_priority_min", min));
        }
        if let Some(max) = priority_max {
            qb = qb.bind(("filter_priority_max", max));
        }
        if let Some(q) = search {
            qb = qb.bind(("filter_search", q.to_lowercase()));
        }

        let mut resp = qb.await.context("Failed to list plans filtered")?;

        let count_result: Vec<serde_json::Value> = resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let records: Vec<PlanRecord> = resp.take(1)?;
        let plans: Vec<PlanNode> = records
            .into_iter()
            .filter_map(|r| r.into_node().ok())
            .collect();

        Ok((plans, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::test_helpers::{test_plan, test_plan_for_project, test_project};

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_create_and_get_plan() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        let retrieved = store.get_plan(plan.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, plan.id);
        assert_eq!(retrieved.title, "Test Plan");
        assert_eq!(retrieved.status, PlanStatus::Draft);
        assert_eq!(retrieved.priority, 5);
    }

    #[tokio::test]
    async fn test_get_nonexistent_plan() {
        let store = setup().await;
        let result = store.get_plan(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_update_plan_status() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        store
            .update_plan_status(plan.id, PlanStatus::InProgress)
            .await
            .unwrap();

        let updated = store.get_plan(plan.id).await.unwrap().unwrap();
        assert_eq!(updated.status, PlanStatus::InProgress);
    }

    #[tokio::test]
    async fn test_delete_plan() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        store.delete_plan(plan.id).await.unwrap();
        let result = store.get_plan(plan.id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_plan_with_project() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let plan = test_plan_for_project(project.id);
        store.create_plan(&plan).await.unwrap();

        let retrieved = store.get_plan(plan.id).await.unwrap().unwrap();
        assert_eq!(retrieved.project_id, Some(project.id));
    }

    #[tokio::test]
    async fn test_link_and_unlink_plan() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        assert!(store
            .get_plan(plan.id)
            .await
            .unwrap()
            .unwrap()
            .project_id
            .is_none());

        store
            .link_plan_to_project(plan.id, project.id)
            .await
            .unwrap();
        let linked = store.get_plan(plan.id).await.unwrap().unwrap();
        assert_eq!(linked.project_id, Some(project.id));

        store.unlink_plan_from_project(plan.id).await.unwrap();
        let unlinked = store.get_plan(plan.id).await.unwrap().unwrap();
        assert!(unlinked.project_id.is_none());
    }

    #[tokio::test]
    async fn test_list_active_plans() {
        let store = setup().await;
        let mut p1 = test_plan();
        p1.title = "Active Plan".to_string();
        let mut p2 = test_plan();
        p2.title = "Completed Plan".to_string();
        p2.status = PlanStatus::Completed;
        let mut p3 = test_plan();
        p3.title = "Approved Plan".to_string();
        p3.status = PlanStatus::Approved;

        store.create_plan(&p1).await.unwrap();
        store.create_plan(&p2).await.unwrap();
        store.create_plan(&p3).await.unwrap();

        let active = store.list_active_plans().await.unwrap();
        assert_eq!(active.len(), 2);
    }

    #[tokio::test]
    async fn test_list_project_plans() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let p1 = test_plan_for_project(project.id);
        let p2 = test_plan_for_project(project.id);
        let p3 = test_plan(); // no project

        store.create_plan(&p1).await.unwrap();
        store.create_plan(&p2).await.unwrap();
        store.create_plan(&p3).await.unwrap();

        let plans = store.list_project_plans(project.id).await.unwrap();
        assert_eq!(plans.len(), 2);
    }

    #[tokio::test]
    async fn test_count_project_plans() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        for _ in 0..3 {
            let p = test_plan_for_project(project.id);
            store.create_plan(&p).await.unwrap();
        }

        let count = store.count_project_plans(project.id).await.unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_list_plans_filtered() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let mut p1 = test_plan_for_project(project.id);
        p1.priority = 10;
        let mut p2 = test_plan_for_project(project.id);
        p2.priority = 3;
        p2.status = PlanStatus::Completed;

        store.create_plan(&p1).await.unwrap();
        store.create_plan(&p2).await.unwrap();

        // Filter by status
        let (plans, total) = store
            .list_plans_filtered(
                Some(project.id),
                None,
                Some(vec!["draft".to_string()]),
                None,
                None,
                None,
                10,
                0,
                None,
                "desc",
            )
            .await
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].priority, 10);
    }
}
