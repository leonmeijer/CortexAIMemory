//! Step CRUD operations for IndentiaGraphStore.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{StepNode, StepStatus};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

#[derive(Debug, SurrealValue)]
struct StepRecord {
    id: RecordId,
    description: String,
    status: String,
    order_idx: i64,
    verification: Option<String>,
    #[allow(dead_code)]
    task_id: String,
    created_at: String,
    #[allow(dead_code)]
    updated_at: Option<String>,
    completed_at: Option<String>,
}

impl StepRecord {
    fn into_node(self) -> Result<StepNode> {
        Ok(StepNode {
            id: rid_to_uuid(&self.id)?,
            order: self.order_idx as u32,
            description: self.description,
            status: parse_step_status(&self.status),
            verification: self.verification,
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            updated_at: None,
            completed_at: self.completed_at.and_then(|s| s.parse().ok()),
        })
    }
}

fn parse_step_status(s: &str) -> StepStatus {
    match s {
        "pending" => StepStatus::Pending,
        "in_progress" => StepStatus::InProgress,
        "completed" => StepStatus::Completed,
        "skipped" => StepStatus::Skipped,
        _ => StepStatus::Pending,
    }
}

fn status_to_str(s: &StepStatus) -> &'static str {
    match s {
        StepStatus::Pending => "pending",
        StepStatus::InProgress => "in_progress",
        StepStatus::Completed => "completed",
        StepStatus::Skipped => "skipped",
    }
}

impl IndentiaGraphStore {
    pub async fn create_step(&self, task_id: Uuid, step: &StepNode) -> Result<()> {
        let rid = RecordId::new("step", step.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 description = $desc, status = $status, \
                 order_idx = $order, verification = $verification, \
                 task_id = $task_id, created_at = $created_at \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("desc", step.description.clone()))
            .bind(("status", status_to_str(&step.status).to_string()))
            .bind(("order", step.order as i64))
            .bind(("verification", step.verification.clone()))
            .bind(("task_id", task_id.to_string()))
            .bind(("created_at", step.created_at.to_rfc3339()))
            .await
            .context("Failed to create step")?;
        Ok(())
    }

    pub async fn get_step(&self, step_id: Uuid) -> Result<Option<StepNode>> {
        let rid = RecordId::new("step", step_id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get step")?;
        let records: Vec<StepRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn get_task_steps(&self, task_id: Uuid) -> Result<Vec<StepNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM step WHERE task_id = $tid ORDER BY order_idx ASC")
            .bind(("tid", task_id.to_string()))
            .await
            .context("Failed to get task steps")?;
        let records: Vec<StepRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn update_step_status(&self, step_id: Uuid, status: StepStatus) -> Result<()> {
        let completed = if status == StepStatus::Completed {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };
        self.db
            .query(
                "UPDATE $rid SET status = $status, updated_at = $now, \
                 completed_at = IF $completed != NONE THEN $completed ELSE completed_at END \
                 RETURN NONE",
            )
            .bind(("rid", RecordId::new("step", step_id.to_string().as_str())))
            .bind(("status", status_to_str(&status).to_string()))
            .bind(("now", Utc::now().to_rfc3339()))
            .bind(("completed", completed))
            .await
            .context("Failed to update step status")?;
        Ok(())
    }

    pub async fn get_task_step_progress(&self, task_id: Uuid) -> Result<(u32, u32)> {
        let steps = self.get_task_steps(task_id).await?;
        let total = steps.len() as u32;
        let completed = steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed || s.status == StepStatus::Skipped)
            .count() as u32;
        Ok((completed, total))
    }

    pub async fn delete_step(&self, step_id: Uuid) -> Result<()> {
        let rid = RecordId::new("step", step_id.to_string().as_str());
        self.db
            .query("DELETE $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to delete step")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::test_helpers::{test_plan, test_step, test_task};

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_create_and_get_step() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Task with steps");
        store.create_task(plan.id, &task).await.unwrap();

        let step = test_step(0, "First step");
        store.create_step(task.id, &step).await.unwrap();

        let retrieved = store.get_step(step.id).await.unwrap().unwrap();
        assert_eq!(retrieved.description, "First step");
        assert_eq!(retrieved.order, 0);
        assert_eq!(retrieved.status, StepStatus::Pending);
    }

    #[tokio::test]
    async fn test_get_task_steps_ordered() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Multi-step task");
        store.create_task(plan.id, &task).await.unwrap();

        let s1 = test_step(0, "Setup");
        let s2 = test_step(1, "Implement");
        let s3 = test_step(2, "Test");
        store.create_step(task.id, &s1).await.unwrap();
        store.create_step(task.id, &s3).await.unwrap(); // out of order
        store.create_step(task.id, &s2).await.unwrap();

        let steps = store.get_task_steps(task.id).await.unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].description, "Setup");
        assert_eq!(steps[1].description, "Implement");
        assert_eq!(steps[2].description, "Test");
    }

    #[tokio::test]
    async fn test_update_step_status() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Task");
        store.create_task(plan.id, &task).await.unwrap();
        let step = test_step(0, "Do something");
        store.create_step(task.id, &step).await.unwrap();

        store
            .update_step_status(step.id, StepStatus::Completed)
            .await
            .unwrap();
        let updated = store.get_step(step.id).await.unwrap().unwrap();
        assert_eq!(updated.status, StepStatus::Completed);
        assert!(updated.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_step_progress() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Task");
        store.create_task(plan.id, &task).await.unwrap();

        for i in 0..4 {
            let step = test_step(i, &format!("Step {}", i));
            store.create_step(task.id, &step).await.unwrap();
            if i < 2 {
                store
                    .update_step_status(step.id, StepStatus::Completed)
                    .await
                    .unwrap();
            }
        }

        let (completed, total) = store.get_task_step_progress(task.id).await.unwrap();
        assert_eq!(total, 4);
        assert_eq!(completed, 2);
    }

    #[tokio::test]
    async fn test_delete_step() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let task = test_task("Task");
        store.create_task(plan.id, &task).await.unwrap();
        let step = test_step(0, "Deletable");
        store.create_step(task.id, &step).await.unwrap();

        store.delete_step(step.id).await.unwrap();
        assert!(store.get_step(step.id).await.unwrap().is_none());
    }
}
