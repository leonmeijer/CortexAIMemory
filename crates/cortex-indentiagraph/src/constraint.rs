//! Constraint CRUD operations for IndentiaGraphStore.

use anyhow::{Context, Result};
use cortex_core::models::{ConstraintNode, ConstraintType};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

#[derive(Debug, SurrealValue)]
struct ConstraintRecord {
    id: RecordId,
    constraint_type: String,
    description: String,
    enforced_by: Option<String>,
    #[allow(dead_code)]
    plan_id: String,
}

impl ConstraintRecord {
    fn into_node(self) -> Result<ConstraintNode> {
        Ok(ConstraintNode {
            id: rid_to_uuid(&self.id)?,
            constraint_type: parse_constraint_type(&self.constraint_type),
            description: self.description,
            enforced_by: self.enforced_by,
        })
    }
}

fn parse_constraint_type(s: &str) -> ConstraintType {
    match s {
        "performance" => ConstraintType::Performance,
        "compatibility" => ConstraintType::Compatibility,
        "security" => ConstraintType::Security,
        "style" => ConstraintType::Style,
        "testing" => ConstraintType::Testing,
        _ => ConstraintType::Other,
    }
}

fn type_to_str(t: &ConstraintType) -> &'static str {
    match t {
        ConstraintType::Performance => "performance",
        ConstraintType::Compatibility => "compatibility",
        ConstraintType::Security => "security",
        ConstraintType::Style => "style",
        ConstraintType::Testing => "testing",
        ConstraintType::Other => "other",
    }
}

impl IndentiaGraphStore {
    pub async fn create_constraint(
        &self,
        plan_id: Uuid,
        constraint: &ConstraintNode,
    ) -> Result<()> {
        let rid = RecordId::new("constraint", constraint.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 constraint_type = $ct, description = $desc, \
                 enforced_by = $eb, plan_id = $plan_id, created_at = $now \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("ct", type_to_str(&constraint.constraint_type).to_string()))
            .bind(("desc", constraint.description.clone()))
            .bind(("eb", constraint.enforced_by.clone()))
            .bind(("plan_id", plan_id.to_string()))
            .bind(("now", chrono::Utc::now().to_rfc3339()))
            .await
            .context("Failed to create constraint")?;
        Ok(())
    }

    pub async fn get_constraint(&self, constraint_id: Uuid) -> Result<Option<ConstraintNode>> {
        let rid = RecordId::new("constraint", constraint_id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get constraint")?;
        let records: Vec<ConstraintRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn get_plan_constraints(&self, plan_id: Uuid) -> Result<Vec<ConstraintNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM constraint WHERE plan_id = $pid")
            .bind(("pid", plan_id.to_string()))
            .await
            .context("Failed to get plan constraints")?;
        let records: Vec<ConstraintRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn update_constraint(
        &self,
        constraint_id: Uuid,
        description: Option<String>,
        constraint_type: Option<ConstraintType>,
        enforced_by: Option<String>,
    ) -> Result<()> {
        let mut sets = Vec::new();
        if description.is_some() {
            sets.push("description = $desc");
        }
        if constraint_type.is_some() {
            sets.push("constraint_type = $ct");
        }
        if enforced_by.is_some() {
            sets.push("enforced_by = $eb");
        }
        if sets.is_empty() {
            return Ok(());
        }

        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let mut q = self.db.query(&query);
        q = q.bind((
            "rid",
            RecordId::new("constraint", constraint_id.to_string().as_str()),
        ));
        if let Some(ref d) = description {
            q = q.bind(("desc", d.clone()));
        }
        if let Some(ref ct) = constraint_type {
            q = q.bind(("ct", type_to_str(ct).to_string()));
        }
        if let Some(ref eb) = enforced_by {
            q = q.bind(("eb", eb.clone()));
        }

        q.await.context("Failed to update constraint")?;
        Ok(())
    }

    pub async fn delete_constraint(&self, constraint_id: Uuid) -> Result<()> {
        let rid = RecordId::new("constraint", constraint_id.to_string().as_str());
        self.db
            .query("DELETE $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to delete constraint")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::test_helpers::{test_constraint, test_plan};

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_create_and_get_constraint() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        let c = test_constraint();
        store.create_constraint(plan.id, &c).await.unwrap();

        let retrieved = store.get_constraint(c.id).await.unwrap().unwrap();
        assert_eq!(retrieved.constraint_type, ConstraintType::Performance);
        assert_eq!(retrieved.description, "Response time under 100ms");
    }

    #[tokio::test]
    async fn test_get_plan_constraints() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();

        let c1 = test_constraint();
        let mut c2 = test_constraint();
        c2.constraint_type = ConstraintType::Security;
        c2.description = "Must sanitize input".to_string();
        store.create_constraint(plan.id, &c1).await.unwrap();
        store.create_constraint(plan.id, &c2).await.unwrap();

        let constraints = store.get_plan_constraints(plan.id).await.unwrap();
        assert_eq!(constraints.len(), 2);
    }

    #[tokio::test]
    async fn test_update_constraint() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let c = test_constraint();
        store.create_constraint(plan.id, &c).await.unwrap();

        store
            .update_constraint(c.id, Some("Updated desc".to_string()), None, None)
            .await
            .unwrap();

        let updated = store.get_constraint(c.id).await.unwrap().unwrap();
        assert_eq!(updated.description, "Updated desc");
        assert_eq!(updated.constraint_type, ConstraintType::Performance);
    }

    #[tokio::test]
    async fn test_delete_constraint() {
        let store = setup().await;
        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let c = test_constraint();
        store.create_constraint(plan.id, &c).await.unwrap();

        store.delete_constraint(c.id).await.unwrap();
        assert!(store.get_constraint(c.id).await.unwrap().is_none());
    }
}
