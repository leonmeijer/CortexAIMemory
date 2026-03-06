//! Workspace, Resource, and Component CRUD operations for IndentiaGraphStore.
//!
//! Implements workspace grouping, cross-project milestones, shared resources,
//! and deployment topology components.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{
    ComponentDependency, ComponentNode, ComponentType, MilestoneStatus, ProjectNode, ResourceNode,
    ResourceType, StepNode, TaskNode, TaskStatus, TaskWithPlan, WorkspaceMilestoneNode,
    WorkspaceNode,
};
use std::collections::HashMap;
use surrealdb::types::{RecordId, RecordIdKey, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};
use crate::task::{json_str_to_vec, parse_task_status};

// ---------------------------------------------------------------------------
// Record types (module-level for SurrealValue derive)
// ---------------------------------------------------------------------------

#[derive(Debug, SurrealValue)]
struct WorkspaceRecord {
    id: RecordId,
    name: String,
    slug: String,
    description: Option<String>,
    created_at: String,
    updated_at: Option<String>,
}

impl WorkspaceRecord {
    fn into_node(self) -> Result<WorkspaceNode> {
        Ok(WorkspaceNode {
            id: rid_to_uuid(&self.id)?,
            name: self.name,
            slug: self.slug,
            description: self.description,
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            updated_at: self.updated_at.and_then(|s| s.parse().ok()),
            metadata: serde_json::Value::Null,
        })
    }
}

#[derive(Debug, SurrealValue)]
struct ProjectRecord {
    id: RecordId,
    name: String,
    slug: String,
    root_path: String,
    description: Option<String>,
    created_at: Option<String>,
    last_synced: Option<String>,
    analytics_computed_at: Option<String>,
    last_co_change_computed_at: Option<String>,
}

impl ProjectRecord {
    fn into_node(self) -> Option<ProjectNode> {
        let key_str = match &self.id.key {
            RecordIdKey::String(s) => s.clone(),
            RecordIdKey::Uuid(u) => u.to_string(),
            other => format!("{:?}", other),
        };
        let raw = key_str.trim_start_matches('⟨').trim_end_matches('⟩');
        let id = Uuid::parse_str(raw).ok()?;

        Some(ProjectNode {
            id,
            name: self.name,
            slug: self.slug,
            root_path: self.root_path,
            description: self.description,
            created_at: self
                .created_at
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(Utc::now),
            last_synced: self.last_synced.and_then(|s| s.parse().ok()),
            analytics_computed_at: self.analytics_computed_at.and_then(|s| s.parse().ok()),
            last_co_change_computed_at: self
                .last_co_change_computed_at
                .and_then(|s| s.parse().ok()),
        })
    }
}

#[derive(Debug, SurrealValue)]
struct WsMilestoneRecord {
    id: RecordId,
    title: String,
    description: Option<String>,
    status: String,
    workspace_id: String,
    target_date: Option<String>,
    closed_at: Option<String>,
    created_at: String,
    #[allow(dead_code)]
    updated_at: Option<String>,
    tags: Option<Vec<String>>,
}

impl WsMilestoneRecord {
    fn into_node(self) -> Result<WorkspaceMilestoneNode> {
        Ok(WorkspaceMilestoneNode {
            id: rid_to_uuid(&self.id)?,
            workspace_id: Uuid::parse_str(&self.workspace_id).unwrap_or_else(|_| Uuid::default()),
            title: self.title,
            description: self.description,
            status: parse_milestone_status(&self.status),
            target_date: self.target_date.and_then(|s| s.parse().ok()),
            closed_at: self.closed_at.and_then(|s| s.parse().ok()),
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            tags: self.tags.unwrap_or_default(),
        })
    }
}

#[derive(Debug, SurrealValue)]
struct ResourceRecord {
    id: RecordId,
    name: String,
    resource_type: String,
    description: Option<String>,
    url: Option<String>,
    file_path: Option<String>,
    format: Option<String>,
    version: Option<String>,
    workspace_id: Option<String>,
    project_id: Option<String>,
    created_at: String,
    updated_at: Option<String>,
}

impl ResourceRecord {
    fn into_node(self) -> Result<ResourceNode> {
        Ok(ResourceNode {
            id: rid_to_uuid(&self.id)?,
            workspace_id: self
                .workspace_id
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok()),
            project_id: self
                .project_id
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok()),
            name: self.name,
            resource_type: parse_resource_type(&self.resource_type),
            file_path: self.file_path.unwrap_or_default(),
            url: self.url,
            format: self.format,
            version: self.version,
            description: self.description,
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            updated_at: self.updated_at.and_then(|s| s.parse().ok()),
            metadata: serde_json::Value::Null,
        })
    }
}

#[derive(Debug, SurrealValue)]
struct ComponentRecord {
    id: RecordId,
    name: String,
    component_type: String,
    description: Option<String>,
    workspace_id: String,
    #[allow(dead_code)]
    project_id: Option<String>,
    runtime: Option<String>,
    tags: Option<Vec<String>>,
    created_at: String,
}

impl ComponentRecord {
    fn into_node(self) -> Result<ComponentNode> {
        Ok(ComponentNode {
            id: rid_to_uuid(&self.id)?,
            workspace_id: Uuid::parse_str(&self.workspace_id).unwrap_or_else(|_| Uuid::default()),
            name: self.name,
            component_type: parse_component_type(&self.component_type),
            description: self.description,
            runtime: self.runtime,
            config: serde_json::Value::Null,
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            tags: self.tags.unwrap_or_default(),
        })
    }
}

#[derive(Debug, SurrealValue)]
struct CompDepRecord {
    out: RecordId,
    protocol: Option<String>,
    required: Option<bool>,
}

/// Task record for milestone queries (duplicated here to avoid import issues).
#[derive(Debug, SurrealValue)]
struct WsTaskRecord {
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

impl WsTaskRecord {
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

/// Record for workspace milestone list with workspace info.
#[derive(Debug, SurrealValue)]
struct WsMilestoneWithInfoRecord {
    id: RecordId,
    title: String,
    description: Option<String>,
    status: String,
    workspace_id: String,
    target_date: Option<String>,
    closed_at: Option<String>,
    created_at: String,
    #[allow(dead_code)]
    updated_at: Option<String>,
    tags: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

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

fn milestone_status_str(s: &MilestoneStatus) -> &'static str {
    match s {
        MilestoneStatus::Planned => "planned",
        MilestoneStatus::Open => "open",
        MilestoneStatus::InProgress => "in_progress",
        MilestoneStatus::Completed => "completed",
        MilestoneStatus::Closed => "closed",
    }
}

fn parse_resource_type(s: &str) -> ResourceType {
    s.parse().unwrap_or(ResourceType::Other)
}

fn resource_type_str(rt: &ResourceType) -> &'static str {
    match rt {
        ResourceType::ApiContract => "api_contract",
        ResourceType::Protobuf => "protobuf",
        ResourceType::GraphqlSchema => "graphql_schema",
        ResourceType::JsonSchema => "json_schema",
        ResourceType::DatabaseSchema => "database_schema",
        ResourceType::SharedTypes => "shared_types",
        ResourceType::Config => "config",
        ResourceType::Documentation => "documentation",
        ResourceType::Other => "other",
    }
}

fn parse_component_type(s: &str) -> ComponentType {
    s.parse().unwrap_or(ComponentType::Other)
}

fn component_type_str(ct: &ComponentType) -> &'static str {
    match ct {
        ComponentType::Service => "service",
        ComponentType::Frontend => "frontend",
        ComponentType::Worker => "worker",
        ComponentType::Database => "database",
        ComponentType::MessageQueue => "message_queue",
        ComponentType::Cache => "cache",
        ComponentType::Gateway => "gateway",
        ComponentType::External => "external",
        ComponentType::Other => "other",
    }
}

// ===========================================================================
// Workspace CRUD
// ===========================================================================

impl IndentiaGraphStore {
    pub async fn create_workspace(&self, ws: &WorkspaceNode) -> Result<()> {
        let rid = RecordId::new("workspace", ws.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 name = $name, slug = $slug, description = $desc, \
                 created_at = $created_at, updated_at = $updated_at \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("name", ws.name.clone()))
            .bind(("slug", ws.slug.clone()))
            .bind(("desc", ws.description.clone()))
            .bind(("created_at", ws.created_at.to_rfc3339()))
            .bind(("updated_at", ws.updated_at.map(|d| d.to_rfc3339())))
            .await
            .context("Failed to create workspace")?;
        Ok(())
    }

    pub async fn get_workspace(&self, id: Uuid) -> Result<Option<WorkspaceNode>> {
        let rid = RecordId::new("workspace", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get workspace")?;
        let records: Vec<WorkspaceRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn get_workspace_by_slug(&self, slug: &str) -> Result<Option<WorkspaceNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM workspace WHERE slug = $slug LIMIT 1")
            .bind(("slug", slug.to_string()))
            .await
            .context("Failed to get workspace by slug")?;
        let records: Vec<WorkspaceRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn list_workspaces(&self) -> Result<Vec<WorkspaceNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM workspace ORDER BY created_at DESC")
            .await
            .context("Failed to list workspaces")?;
        let records: Vec<WorkspaceRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn update_workspace(
        &self,
        id: Uuid,
        name: Option<String>,
        description: Option<String>,
        _metadata: Option<serde_json::Value>,
    ) -> Result<()> {
        let mut sets = vec!["updated_at = $now".to_string()];
        if name.is_some() {
            sets.push("name = $name".to_string());
        }
        if description.is_some() {
            sets.push("description = $desc".to_string());
        }

        let rid = RecordId::new("workspace", id.to_string().as_str());
        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let mut q = self.db.query(&query);
        q = q.bind(("rid", rid)).bind(("now", Utc::now().to_rfc3339()));
        if let Some(ref n) = name {
            q = q.bind(("name", n.clone()));
        }
        if let Some(ref d) = description {
            q = q.bind(("desc", d.clone()));
        }
        q.await.context("Failed to update workspace")?;
        Ok(())
    }

    pub async fn delete_workspace(&self, id: Uuid) -> Result<()> {
        let rid = RecordId::new("workspace", id.to_string().as_str());
        self.db
            .query("DELETE $rid RETURN NONE")
            .bind(("rid", rid))
            .await
            .context("Failed to delete workspace")?;
        Ok(())
    }

    // =========================================================================
    // Workspace-Project associations
    // =========================================================================

    pub async fn add_project_to_workspace(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
    ) -> Result<()> {
        let p_rid = RecordId::new("project", project_id.to_string().as_str());
        let w_rid = RecordId::new("workspace", workspace_id.to_string().as_str());
        self.db
            .query("RELATE $from->belongs_to_workspace->$to RETURN NONE")
            .bind(("from", p_rid))
            .bind(("to", w_rid))
            .await
            .context("Failed to add project to workspace")?;
        Ok(())
    }

    pub async fn remove_project_from_workspace(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
    ) -> Result<()> {
        let pid = project_id.to_string();
        let wid = workspace_id.to_string();
        self.db
            .query(
                "DELETE FROM belongs_to_workspace WHERE \
                 in = type::record('project', $pid) AND \
                 out = type::record('workspace', $wid)",
            )
            .bind(("pid", pid))
            .bind(("wid", wid))
            .await
            .context("Failed to remove project from workspace")?;
        Ok(())
    }

    pub async fn list_workspace_projects(&self, workspace_id: Uuid) -> Result<Vec<ProjectNode>> {
        let wid = workspace_id.to_string();
        let mut resp = self
            .db
            .query(
                "SELECT * FROM project WHERE id IN \
                 (SELECT VALUE in.id FROM belongs_to_workspace \
                  WHERE out = type::record('workspace', $wid))",
            )
            .bind(("wid", wid))
            .await
            .context("Failed to list workspace projects")?;
        let records: Vec<ProjectRecord> = resp.take(0)?;
        Ok(records.into_iter().filter_map(|r| r.into_node()).collect())
    }

    pub async fn get_project_workspace(&self, project_id: Uuid) -> Result<Option<WorkspaceNode>> {
        let pid = project_id.to_string();
        let mut resp = self
            .db
            .query(
                "SELECT * FROM workspace WHERE id IN \
                 (SELECT VALUE out.id FROM belongs_to_workspace \
                  WHERE in = type::record('project', $pid)) LIMIT 1",
            )
            .bind(("pid", pid))
            .await
            .context("Failed to get project workspace")?;
        let records: Vec<WorkspaceRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    // =========================================================================
    // Workspace Milestones
    // =========================================================================

    pub async fn create_workspace_milestone(
        &self,
        milestone: &WorkspaceMilestoneNode,
    ) -> Result<()> {
        let rid = RecordId::new("workspace_milestone", milestone.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 title = $title, description = $desc, \
                 status = $status, workspace_id = $wid, \
                 target_date = $td, closed_at = $ca, \
                 created_at = $created_at, tags = $tags \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("title", milestone.title.clone()))
            .bind(("desc", milestone.description.clone()))
            .bind((
                "status",
                milestone_status_str(&milestone.status).to_string(),
            ))
            .bind(("wid", milestone.workspace_id.to_string()))
            .bind(("td", milestone.target_date.map(|d| d.to_rfc3339())))
            .bind(("ca", milestone.closed_at.map(|d| d.to_rfc3339())))
            .bind(("created_at", milestone.created_at.to_rfc3339()))
            .bind(("tags", milestone.tags.clone()))
            .await
            .context("Failed to create workspace milestone")?;
        Ok(())
    }

    pub async fn get_workspace_milestone(
        &self,
        id: Uuid,
    ) -> Result<Option<WorkspaceMilestoneNode>> {
        let rid = RecordId::new("workspace_milestone", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get workspace milestone")?;
        let records: Vec<WsMilestoneRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn list_workspace_milestones(
        &self,
        workspace_id: Uuid,
    ) -> Result<Vec<WorkspaceMilestoneNode>> {
        let wid = workspace_id.to_string();
        let mut resp = self
            .db
            .query(
                "SELECT * FROM workspace_milestone WHERE workspace_id = $wid \
                 ORDER BY created_at DESC",
            )
            .bind(("wid", wid))
            .await
            .context("Failed to list workspace milestones")?;
        let records: Vec<WsMilestoneRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn list_workspace_milestones_filtered(
        &self,
        workspace_id: Uuid,
        status: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<WorkspaceMilestoneNode>, usize)> {
        let wid = workspace_id.to_string();
        let mut conditions = vec![format!("workspace_id = '{}'", wid)];
        if let Some(s) = status {
            conditions.push(format!("status = '{}'", s));
        }
        let where_clause = format!("WHERE {}", conditions.join(" AND "));

        let count_q = format!(
            "SELECT count() AS total FROM workspace_milestone {} GROUP ALL",
            where_clause
        );
        let data_q = format!(
            "SELECT * FROM workspace_milestone {} ORDER BY created_at DESC LIMIT {} START {}",
            where_clause, limit, offset
        );

        let mut resp = self
            .db
            .query(format!("{}; {}", count_q, data_q))
            .await
            .context("Failed to list workspace milestones filtered")?;
        let count_result: Vec<serde_json::Value> = resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let records: Vec<WsMilestoneRecord> = resp.take(1)?;
        let milestones = records
            .into_iter()
            .filter_map(|r| r.into_node().ok())
            .collect();
        Ok((milestones, total))
    }

    pub async fn list_all_workspace_milestones_filtered(
        &self,
        workspace_id: Option<Uuid>,
        status: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(WorkspaceMilestoneNode, String, String, String)>> {
        let mut conditions = Vec::new();
        if let Some(wid) = workspace_id {
            conditions.push(format!("workspace_id = '{}'", wid));
        }
        if let Some(s) = status {
            conditions.push(format!("status = '{}'", s));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let data_q = format!(
            "SELECT * FROM workspace_milestone {} ORDER BY created_at DESC LIMIT {} START {}",
            where_clause, limit, offset
        );
        let mut resp = self
            .db
            .query(&data_q)
            .await
            .context("Failed to list all workspace milestones filtered")?;
        let records: Vec<WsMilestoneWithInfoRecord> = resp.take(0)?;

        let mut result = Vec::new();
        for rec in records {
            let wid = rec.workspace_id.clone();
            let ms = WorkspaceMilestoneNode {
                id: rid_to_uuid(&rec.id)?,
                workspace_id: Uuid::parse_str(&rec.workspace_id)
                    .unwrap_or_else(|_| Uuid::default()),
                title: rec.title,
                description: rec.description,
                status: parse_milestone_status(&rec.status),
                target_date: rec.target_date.and_then(|s| s.parse().ok()),
                closed_at: rec.closed_at.and_then(|s| s.parse().ok()),
                created_at: rec
                    .created_at
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                tags: rec.tags.unwrap_or_default(),
            };

            // Look up workspace name and slug
            let ws_rid = RecordId::new("workspace", wid.as_str());
            let mut ws_resp = self
                .db
                .query("SELECT name, slug FROM $rid")
                .bind(("rid", ws_rid))
                .await?;
            let ws_info: Vec<serde_json::Value> = ws_resp.take(0)?;
            let (ws_name, ws_slug) = if let Some(info) = ws_info.first() {
                (
                    info.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown")
                        .to_string(),
                    info.get("slug")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                )
            } else {
                ("Unknown".to_string(), "unknown".to_string())
            };

            result.push((ms, wid, ws_name, ws_slug));
        }
        Ok(result)
    }

    pub async fn count_all_workspace_milestones(
        &self,
        workspace_id: Option<Uuid>,
        status: Option<&str>,
    ) -> Result<usize> {
        let mut conditions = Vec::new();
        if let Some(wid) = workspace_id {
            conditions.push(format!("workspace_id = '{}'", wid));
        }
        if let Some(s) = status {
            conditions.push(format!("status = '{}'", s));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let q = format!(
            "SELECT count() AS total FROM workspace_milestone {} GROUP ALL",
            where_clause
        );
        let mut resp = self.db.query(&q).await?;
        let count_result: Vec<serde_json::Value> = resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        Ok(total)
    }

    pub async fn update_workspace_milestone(
        &self,
        id: Uuid,
        title: Option<String>,
        description: Option<String>,
        status: Option<MilestoneStatus>,
        target_date: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let mut sets = vec!["updated_at = $now".to_string()];
        if title.is_some() {
            sets.push("title = $title".to_string());
        }
        if description.is_some() {
            sets.push("description = $desc".to_string());
        }
        if status.is_some() {
            sets.push("status = $status".to_string());
        }
        if target_date.is_some() {
            sets.push("target_date = $td".to_string());
        }

        let rid = RecordId::new("workspace_milestone", id.to_string().as_str());
        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let mut q = self.db.query(&query);
        q = q.bind(("rid", rid)).bind(("now", Utc::now().to_rfc3339()));
        if let Some(ref t) = title {
            q = q.bind(("title", t.clone()));
        }
        if let Some(ref d) = description {
            q = q.bind(("desc", d.clone()));
        }
        if let Some(ref s) = status {
            q = q.bind(("status", milestone_status_str(s).to_string()));
        }
        if let Some(d) = target_date {
            q = q.bind(("td", d.to_rfc3339()));
        }
        q.await.context("Failed to update workspace milestone")?;
        Ok(())
    }

    pub async fn delete_workspace_milestone(&self, id: Uuid) -> Result<()> {
        let mid = id.to_string();
        let rid = RecordId::new("workspace_milestone", mid.as_str());
        self.db
            .query(
                "DELETE FROM includes_task WHERE in = type::record('workspace_milestone', $mid); \
                 DELETE $rid",
            )
            .bind(("mid", mid))
            .bind(("rid", rid))
            .await
            .context("Failed to delete workspace milestone")?;
        Ok(())
    }

    pub async fn add_task_to_workspace_milestone(
        &self,
        milestone_id: Uuid,
        task_id: Uuid,
    ) -> Result<()> {
        let ms_rid = RecordId::new("workspace_milestone", milestone_id.to_string().as_str());
        let task_rid = RecordId::new("task", task_id.to_string().as_str());
        self.db
            .query("RELATE $from->includes_task->$to RETURN NONE")
            .bind(("from", ms_rid))
            .bind(("to", task_rid))
            .await
            .context("Failed to add task to workspace milestone")?;
        Ok(())
    }

    pub async fn remove_task_from_workspace_milestone(
        &self,
        milestone_id: Uuid,
        task_id: Uuid,
    ) -> Result<()> {
        let mid = milestone_id.to_string();
        let tid = task_id.to_string();
        self.db
            .query(
                "DELETE FROM includes_task WHERE \
                 in = type::record('workspace_milestone', $mid) AND \
                 out = type::record('task', $tid)",
            )
            .bind(("mid", mid))
            .bind(("tid", tid))
            .await
            .context("Failed to remove task from workspace milestone")?;
        Ok(())
    }

    pub async fn link_plan_to_workspace_milestone(
        &self,
        plan_id: Uuid,
        milestone_id: Uuid,
    ) -> Result<()> {
        let ms_rid = RecordId::new("workspace_milestone", milestone_id.to_string().as_str());
        let plan_rid = RecordId::new("plan", plan_id.to_string().as_str());
        self.db
            .query("RELATE $from->includes_task->$to RETURN NONE")
            .bind(("from", ms_rid))
            .bind(("to", plan_rid))
            .await
            .context("Failed to link plan to workspace milestone")?;
        Ok(())
    }

    pub async fn unlink_plan_from_workspace_milestone(
        &self,
        plan_id: Uuid,
        milestone_id: Uuid,
    ) -> Result<()> {
        let mid = milestone_id.to_string();
        let pid = plan_id.to_string();
        self.db
            .query(
                "DELETE FROM includes_task WHERE \
                 in = type::record('workspace_milestone', $mid) AND \
                 out = type::record('plan', $pid)",
            )
            .bind(("mid", mid))
            .bind(("pid", pid))
            .await
            .context("Failed to unlink plan from workspace milestone")?;
        Ok(())
    }

    pub async fn get_workspace_milestone_tasks(
        &self,
        milestone_id: Uuid,
    ) -> Result<Vec<TaskWithPlan>> {
        let mid = milestone_id.to_string();
        let query = "SELECT * FROM task WHERE id IN \
             (SELECT VALUE out.id FROM includes_task \
              WHERE in = type::record('workspace_milestone', $mid))";
        let mut resp = self
            .db
            .query(query)
            .bind(("mid", mid))
            .await
            .context("Failed to get workspace milestone tasks")?;
        let records: Vec<WsTaskRecord> = resp.take(0)?;

        let mut result = Vec::new();
        for rec in records {
            let task = rec.into_node()?;
            // Look up plan for this task
            let mut plan_resp = self
                .db
                .query(
                    "SELECT VALUE plan_id FROM task WHERE id = type::record('task', $tid) LIMIT 1",
                )
                .bind(("tid", task.id.to_string()))
                .await?;
            let plan_ids: Vec<Option<String>> = plan_resp.take(0)?;
            let (plan_id, plan_title, plan_status) =
                if let Some(Some(pid_str)) = plan_ids.into_iter().next() {
                    let plan_rid = RecordId::new("plan", pid_str.as_str());
                    let mut pr = self
                        .db
                        .query("SELECT title, status FROM $rid")
                        .bind(("rid", plan_rid))
                        .await?;
                    let plan_info: Vec<serde_json::Value> = pr.take(0)?;
                    if let Some(p) = plan_info.first() {
                        (
                            Uuid::parse_str(&pid_str).unwrap_or_default(),
                            p.get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown")
                                .to_string(),
                            p.get("status")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                        )
                    } else {
                        (
                            Uuid::parse_str(&pid_str).unwrap_or_default(),
                            "Unknown".to_string(),
                            None,
                        )
                    }
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

    pub async fn get_workspace_milestone_progress(
        &self,
        milestone_id: Uuid,
    ) -> Result<(u32, u32, u32, u32)> {
        let tasks = self.get_workspace_milestone_tasks(milestone_id).await?;
        let total = tasks.len() as u32;
        let completed = tasks
            .iter()
            .filter(|t| t.task.status == TaskStatus::Completed)
            .count() as u32;
        let in_progress = tasks
            .iter()
            .filter(|t| t.task.status == TaskStatus::InProgress)
            .count() as u32;
        let pending = tasks
            .iter()
            .filter(|t| t.task.status == TaskStatus::Pending)
            .count() as u32;
        Ok((total, completed, in_progress, pending))
    }

    pub async fn get_workspace_milestone_steps(
        &self,
        milestone_id: Uuid,
    ) -> Result<HashMap<Uuid, Vec<StepNode>>> {
        let tasks = self.get_workspace_milestone_tasks(milestone_id).await?;
        let mut result = HashMap::new();
        for twp in &tasks {
            let steps = self.get_task_steps(twp.task.id).await?;
            result.insert(twp.task.id, steps);
        }
        Ok(result)
    }

    // =========================================================================
    // Resource CRUD
    // =========================================================================

    pub async fn create_resource(&self, resource: &ResourceNode) -> Result<()> {
        let rid = RecordId::new("resource", resource.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 name = $name, resource_type = $rt, description = $desc, \
                 url = $url, file_path = $fp, format = $fmt, version = $ver, \
                 workspace_id = $wid, project_id = $pid, \
                 created_at = $created_at, updated_at = $updated_at \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("name", resource.name.clone()))
            .bind(("rt", resource_type_str(&resource.resource_type).to_string()))
            .bind(("desc", resource.description.clone()))
            .bind(("url", resource.url.clone()))
            .bind(("fp", Some(resource.file_path.clone())))
            .bind(("fmt", resource.format.clone()))
            .bind(("ver", resource.version.clone()))
            .bind(("wid", resource.workspace_id.map(|u| u.to_string())))
            .bind(("pid", resource.project_id.map(|u| u.to_string())))
            .bind(("created_at", resource.created_at.to_rfc3339()))
            .bind(("updated_at", resource.updated_at.map(|d| d.to_rfc3339())))
            .await
            .context("Failed to create resource")?;
        Ok(())
    }

    pub async fn get_resource(&self, id: Uuid) -> Result<Option<ResourceNode>> {
        let rid = RecordId::new("resource", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get resource")?;
        let records: Vec<ResourceRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn list_workspace_resources(&self, workspace_id: Uuid) -> Result<Vec<ResourceNode>> {
        let wid = workspace_id.to_string();
        let mut resp = self
            .db
            .query("SELECT * FROM resource WHERE workspace_id = $wid ORDER BY created_at DESC")
            .bind(("wid", wid))
            .await
            .context("Failed to list workspace resources")?;
        let records: Vec<ResourceRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn update_resource(
        &self,
        id: Uuid,
        name: Option<String>,
        file_path: Option<String>,
        url: Option<String>,
        version: Option<String>,
        description: Option<String>,
    ) -> Result<()> {
        let mut sets = vec!["updated_at = $now".to_string()];
        if name.is_some() {
            sets.push("name = $name".to_string());
        }
        if file_path.is_some() {
            sets.push("file_path = $fp".to_string());
        }
        if url.is_some() {
            sets.push("url = $url".to_string());
        }
        if version.is_some() {
            sets.push("version = $ver".to_string());
        }
        if description.is_some() {
            sets.push("description = $desc".to_string());
        }

        let rid = RecordId::new("resource", id.to_string().as_str());
        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let mut q = self.db.query(&query);
        q = q.bind(("rid", rid)).bind(("now", Utc::now().to_rfc3339()));
        if let Some(ref n) = name {
            q = q.bind(("name", n.clone()));
        }
        if let Some(ref fp) = file_path {
            q = q.bind(("fp", fp.clone()));
        }
        if let Some(ref u) = url {
            q = q.bind(("url", u.clone()));
        }
        if let Some(ref v) = version {
            q = q.bind(("ver", v.clone()));
        }
        if let Some(ref d) = description {
            q = q.bind(("desc", d.clone()));
        }
        q.await.context("Failed to update resource")?;
        Ok(())
    }

    pub async fn delete_resource(&self, id: Uuid) -> Result<()> {
        let sid = id.to_string();
        let rid = RecordId::new("resource", sid.as_str());
        self.db
            .query(
                "DELETE FROM implements_resource WHERE out = type::record('resource', $sid); \
                 DELETE FROM uses_resource WHERE out = type::record('resource', $sid); \
                 DELETE $rid",
            )
            .bind(("sid", sid))
            .bind(("rid", rid))
            .await
            .context("Failed to delete resource")?;
        Ok(())
    }

    pub async fn link_project_implements_resource(
        &self,
        project_id: Uuid,
        resource_id: Uuid,
    ) -> Result<()> {
        let p_rid = RecordId::new("project", project_id.to_string().as_str());
        let r_rid = RecordId::new("resource", resource_id.to_string().as_str());
        self.db
            .query("RELATE $from->implements_resource->$to RETURN NONE")
            .bind(("from", p_rid))
            .bind(("to", r_rid))
            .await
            .context("Failed to link project implements resource")?;
        Ok(())
    }

    pub async fn link_project_uses_resource(
        &self,
        project_id: Uuid,
        resource_id: Uuid,
    ) -> Result<()> {
        let p_rid = RecordId::new("project", project_id.to_string().as_str());
        let r_rid = RecordId::new("resource", resource_id.to_string().as_str());
        self.db
            .query("RELATE $from->uses_resource->$to RETURN NONE")
            .bind(("from", p_rid))
            .bind(("to", r_rid))
            .await
            .context("Failed to link project uses resource")?;
        Ok(())
    }

    pub async fn get_resource_implementers(&self, resource_id: Uuid) -> Result<Vec<ProjectNode>> {
        let resid = resource_id.to_string();
        let mut resp = self
            .db
            .query(
                "SELECT * FROM project WHERE id IN \
                 (SELECT VALUE in.id FROM implements_resource \
                  WHERE out = type::record('resource', $resid))",
            )
            .bind(("resid", resid))
            .await
            .context("Failed to get resource implementers")?;
        let records: Vec<ProjectRecord> = resp.take(0)?;
        Ok(records.into_iter().filter_map(|r| r.into_node()).collect())
    }

    pub async fn get_resource_consumers(&self, resource_id: Uuid) -> Result<Vec<ProjectNode>> {
        let resid = resource_id.to_string();
        let mut resp = self
            .db
            .query(
                "SELECT * FROM project WHERE id IN \
                 (SELECT VALUE in.id FROM uses_resource \
                  WHERE out = type::record('resource', $resid))",
            )
            .bind(("resid", resid))
            .await
            .context("Failed to get resource consumers")?;
        let records: Vec<ProjectRecord> = resp.take(0)?;
        Ok(records.into_iter().filter_map(|r| r.into_node()).collect())
    }

    // =========================================================================
    // Component CRUD
    // =========================================================================

    pub async fn create_component(&self, component: &ComponentNode) -> Result<()> {
        let rid = RecordId::new("component", component.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 name = $name, component_type = $ct, description = $desc, \
                 workspace_id = $wid, runtime = $runtime, \
                 tags = $tags, created_at = $created_at \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("name", component.name.clone()))
            .bind((
                "ct",
                component_type_str(&component.component_type).to_string(),
            ))
            .bind(("desc", component.description.clone()))
            .bind(("wid", component.workspace_id.to_string()))
            .bind(("runtime", component.runtime.clone()))
            .bind(("tags", component.tags.clone()))
            .bind(("created_at", component.created_at.to_rfc3339()))
            .await
            .context("Failed to create component")?;
        Ok(())
    }

    pub async fn get_component(&self, id: Uuid) -> Result<Option<ComponentNode>> {
        let rid = RecordId::new("component", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get component")?;
        let records: Vec<ComponentRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn list_components(&self, workspace_id: Uuid) -> Result<Vec<ComponentNode>> {
        let wid = workspace_id.to_string();
        let mut resp = self
            .db
            .query("SELECT * FROM component WHERE workspace_id = $wid ORDER BY created_at DESC")
            .bind(("wid", wid))
            .await
            .context("Failed to list components")?;
        let records: Vec<ComponentRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn update_component(
        &self,
        id: Uuid,
        name: Option<String>,
        description: Option<String>,
        runtime: Option<String>,
        _config: Option<serde_json::Value>,
        tags: Option<Vec<String>>,
    ) -> Result<()> {
        let mut sets = Vec::new();
        if name.is_some() {
            sets.push("name = $name".to_string());
        }
        if description.is_some() {
            sets.push("description = $desc".to_string());
        }
        if runtime.is_some() {
            sets.push("runtime = $runtime".to_string());
        }
        if tags.is_some() {
            sets.push("tags = $tags".to_string());
        }
        if sets.is_empty() {
            return Ok(());
        }

        let rid = RecordId::new("component", id.to_string().as_str());
        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let mut q = self.db.query(&query).bind(("rid", rid));
        if let Some(ref n) = name {
            q = q.bind(("name", n.clone()));
        }
        if let Some(ref d) = description {
            q = q.bind(("desc", d.clone()));
        }
        if let Some(ref r) = runtime {
            q = q.bind(("runtime", r.clone()));
        }
        if let Some(ref t) = tags {
            q = q.bind(("tags", t.clone()));
        }
        q.await.context("Failed to update component")?;
        Ok(())
    }

    pub async fn delete_component(&self, id: Uuid) -> Result<()> {
        let sid = id.to_string();
        let rid = RecordId::new("component", sid.as_str());
        self.db
            .query(
                "DELETE FROM depends_on_component WHERE in = type::record('component', $sid) OR out = type::record('component', $sid); \
                 DELETE FROM maps_to_project WHERE in = type::record('component', $sid); \
                 DELETE $rid",
            )
            .bind(("sid", sid))
            .bind(("rid", rid))
            .await
            .context("Failed to delete component")?;
        Ok(())
    }

    pub async fn add_component_dependency(
        &self,
        component_id: Uuid,
        depends_on_id: Uuid,
        protocol: Option<String>,
        required: bool,
    ) -> Result<()> {
        let from_rid = RecordId::new("component", component_id.to_string().as_str());
        let to_rid = RecordId::new("component", depends_on_id.to_string().as_str());
        self.db
            .query(
                "RELATE $from->depends_on_component->$to \
                 SET protocol = $protocol, required = $required \
                 RETURN NONE",
            )
            .bind(("from", from_rid))
            .bind(("to", to_rid))
            .bind(("protocol", protocol))
            .bind(("required", required))
            .await
            .context("Failed to add component dependency")?;
        Ok(())
    }

    pub async fn remove_component_dependency(
        &self,
        component_id: Uuid,
        depends_on_id: Uuid,
    ) -> Result<()> {
        let cid = component_id.to_string();
        let did = depends_on_id.to_string();
        self.db
            .query(
                "DELETE FROM depends_on_component WHERE \
                 in = type::record('component', $cid) AND \
                 out = type::record('component', $did)",
            )
            .bind(("cid", cid))
            .bind(("did", did))
            .await
            .context("Failed to remove component dependency")?;
        Ok(())
    }

    pub async fn map_component_to_project(
        &self,
        component_id: Uuid,
        project_id: Uuid,
    ) -> Result<()> {
        let c_rid = RecordId::new("component", component_id.to_string().as_str());
        let p_rid = RecordId::new("project", project_id.to_string().as_str());
        // First remove existing mapping
        let cid = component_id.to_string();
        self.db
            .query(
                "DELETE FROM maps_to_project WHERE in = type::record('component', $cid); \
                 RELATE $from->maps_to_project->$to RETURN NONE",
            )
            .bind(("cid", cid))
            .bind(("from", c_rid))
            .bind(("to", p_rid))
            .await
            .context("Failed to map component to project")?;
        Ok(())
    }

    pub async fn get_workspace_topology(
        &self,
        workspace_id: Uuid,
    ) -> Result<Vec<(ComponentNode, Option<String>, Vec<ComponentDependency>)>> {
        let components = self.list_components(workspace_id).await?;
        let mut result = Vec::new();
        for comp in components {
            // Get mapped project slug
            let cid = comp.id.to_string();
            let mut proj_resp = self
                .db
                .query(
                    "SELECT VALUE out.slug FROM maps_to_project \
                     WHERE in = type::record('component', $cid) LIMIT 1",
                )
                .bind(("cid", cid.clone()))
                .await?;
            let proj_slugs: Vec<Option<String>> = proj_resp.take(0)?;
            let project_slug = proj_slugs.into_iter().next().flatten();

            // Get dependencies
            let mut dep_resp = self
                .db
                .query(
                    "SELECT out, protocol, required FROM depends_on_component \
                     WHERE in = type::record('component', $cid)",
                )
                .bind(("cid", cid))
                .await?;
            let dep_records: Vec<CompDepRecord> = dep_resp.take(0)?;
            let mut deps = Vec::new();
            for dep_rec in dep_records {
                let target_id = rid_to_uuid(&dep_rec.out)?;
                // Look up target name
                let t_rid = RecordId::new("component", target_id.to_string().as_str());
                let mut name_resp = self
                    .db
                    .query("SELECT VALUE name FROM $rid")
                    .bind(("rid", t_rid))
                    .await?;
                let names: Vec<Option<String>> = name_resp.take(0)?;
                let _target_name = names
                    .into_iter()
                    .next()
                    .flatten()
                    .unwrap_or_else(|| "Unknown".to_string());
                deps.push(ComponentDependency {
                    from_id: comp.id,
                    to_id: target_id,
                    protocol: dep_rec.protocol,
                    required: dep_rec.required.unwrap_or(true),
                });
            }
            result.push((comp, project_slug, deps));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use cortex_core::test_helpers::{test_plan, test_project, test_project_named, test_task};

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    fn test_workspace(name: &str) -> WorkspaceNode {
        WorkspaceNode {
            id: Uuid::new_v4(),
            name: name.to_string(),
            slug: name.to_lowercase().replace(' ', "-"),
            description: Some(format!("Test workspace: {}", name)),
            created_at: Utc::now(),
            updated_at: None,
            metadata: serde_json::Value::Null,
        }
    }

    fn test_ws_milestone(workspace_id: Uuid) -> WorkspaceMilestoneNode {
        WorkspaceMilestoneNode {
            id: Uuid::new_v4(),
            workspace_id,
            title: "Cross-project milestone".to_string(),
            description: Some("A milestone spanning projects".to_string()),
            status: MilestoneStatus::Open,
            target_date: None,
            closed_at: None,
            created_at: Utc::now(),
            tags: vec![],
        }
    }

    fn test_resource(workspace_id: Uuid) -> ResourceNode {
        ResourceNode {
            id: Uuid::new_v4(),
            workspace_id: Some(workspace_id),
            project_id: None,
            name: "API Contract".to_string(),
            resource_type: ResourceType::ApiContract,
            file_path: "api/openapi.yaml".to_string(),
            url: None,
            format: Some("openapi".to_string()),
            version: Some("1.0.0".to_string()),
            description: Some("Main API contract".to_string()),
            created_at: Utc::now(),
            updated_at: None,
            metadata: serde_json::Value::Null,
        }
    }

    fn test_component(workspace_id: Uuid, name: &str) -> ComponentNode {
        ComponentNode {
            id: Uuid::new_v4(),
            workspace_id,
            name: name.to_string(),
            component_type: ComponentType::Service,
            description: Some(format!("{} service", name)),
            runtime: Some("docker".to_string()),
            config: serde_json::Value::Null,
            created_at: Utc::now(),
            tags: vec!["rust".to_string()],
        }
    }

    // =========================================================================
    // Workspace CRUD tests
    // =========================================================================

    #[tokio::test]
    async fn test_create_and_get_workspace() {
        let store = setup().await;
        let ws = test_workspace("My Workspace");
        store.create_workspace(&ws).await.unwrap();

        let retrieved = store.get_workspace(ws.id).await.unwrap().unwrap();
        assert_eq!(retrieved.name, "My Workspace");
        assert_eq!(retrieved.slug, ws.slug);
    }

    #[tokio::test]
    async fn test_get_workspace_by_slug() {
        let store = setup().await;
        let ws = test_workspace("Indentia Platform");
        store.create_workspace(&ws).await.unwrap();

        let found = store
            .get_workspace_by_slug(&ws.slug)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, ws.id);
    }

    #[tokio::test]
    async fn test_list_workspaces() {
        let store = setup().await;
        let ws1 = test_workspace("Alpha");
        let ws2 = test_workspace("Beta");
        store.create_workspace(&ws1).await.unwrap();
        store.create_workspace(&ws2).await.unwrap();

        let all = store.list_workspaces().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_update_workspace() {
        let store = setup().await;
        let ws = test_workspace("Old Name");
        store.create_workspace(&ws).await.unwrap();

        store
            .update_workspace(
                ws.id,
                Some("New Name".to_string()),
                Some("Updated description".to_string()),
                None,
            )
            .await
            .unwrap();

        let updated = store.get_workspace(ws.id).await.unwrap().unwrap();
        assert_eq!(updated.name, "New Name");
        assert_eq!(updated.description, Some("Updated description".to_string()));
    }

    #[tokio::test]
    async fn test_delete_workspace() {
        let store = setup().await;
        let ws = test_workspace("Doomed");
        store.create_workspace(&ws).await.unwrap();

        store.delete_workspace(ws.id).await.unwrap();
        assert!(store.get_workspace(ws.id).await.unwrap().is_none());
    }

    // =========================================================================
    // Workspace-Project tests
    // =========================================================================

    #[tokio::test]
    async fn test_workspace_project_lifecycle() {
        let store = setup().await;
        let ws = test_workspace("Workspace");
        store.create_workspace(&ws).await.unwrap();
        let p1 = test_project_named("Project A");
        let p2 = test_project_named("Project B");
        store.create_project(&p1).await.unwrap();
        store.create_project(&p2).await.unwrap();

        store.add_project_to_workspace(ws.id, p1.id).await.unwrap();
        store.add_project_to_workspace(ws.id, p2.id).await.unwrap();

        let projects = store.list_workspace_projects(ws.id).await.unwrap();
        assert_eq!(projects.len(), 2);

        // Check reverse lookup
        let found_ws = store.get_project_workspace(p1.id).await.unwrap().unwrap();
        assert_eq!(found_ws.id, ws.id);

        // Remove one project
        store
            .remove_project_from_workspace(ws.id, p1.id)
            .await
            .unwrap();
        let projects = store.list_workspace_projects(ws.id).await.unwrap();
        assert_eq!(projects.len(), 1);
    }

    // =========================================================================
    // Workspace Milestone tests
    // =========================================================================

    #[tokio::test]
    async fn test_workspace_milestone_crud() {
        let store = setup().await;
        let ws = test_workspace("WS");
        store.create_workspace(&ws).await.unwrap();
        let ms = test_ws_milestone(ws.id);
        store.create_workspace_milestone(&ms).await.unwrap();

        let retrieved = store.get_workspace_milestone(ms.id).await.unwrap().unwrap();
        assert_eq!(retrieved.title, "Cross-project milestone");
        assert_eq!(retrieved.status, MilestoneStatus::Open);

        // Update
        store
            .update_workspace_milestone(
                ms.id,
                Some("Updated title".to_string()),
                None,
                Some(MilestoneStatus::InProgress),
                None,
            )
            .await
            .unwrap();
        let updated = store.get_workspace_milestone(ms.id).await.unwrap().unwrap();
        assert_eq!(updated.title, "Updated title");
        assert_eq!(updated.status, MilestoneStatus::InProgress);

        // Delete
        store.delete_workspace_milestone(ms.id).await.unwrap();
        assert!(store
            .get_workspace_milestone(ms.id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_workspace_milestone_with_tasks() {
        let store = setup().await;
        let ws = test_workspace("WS");
        store.create_workspace(&ws).await.unwrap();
        let ms = test_ws_milestone(ws.id);
        store.create_workspace_milestone(&ms).await.unwrap();

        let plan = test_plan();
        store.create_plan(&plan).await.unwrap();
        let t1 = test_task("Task 1");
        let mut t2 = test_task("Task 2");
        t2.status = TaskStatus::Completed;
        store.create_task(plan.id, &t1).await.unwrap();
        store.create_task(plan.id, &t2).await.unwrap();

        store
            .add_task_to_workspace_milestone(ms.id, t1.id)
            .await
            .unwrap();
        store
            .add_task_to_workspace_milestone(ms.id, t2.id)
            .await
            .unwrap();

        let (total, completed, _, _) = store.get_workspace_milestone_progress(ms.id).await.unwrap();
        assert_eq!(total, 2);
        assert_eq!(completed, 1);

        // Remove task
        store
            .remove_task_from_workspace_milestone(ms.id, t1.id)
            .await
            .unwrap();
        let (total2, _, _, _) = store.get_workspace_milestone_progress(ms.id).await.unwrap();
        assert_eq!(total2, 1);
    }

    // =========================================================================
    // Resource tests
    // =========================================================================

    #[tokio::test]
    async fn test_resource_crud() {
        let store = setup().await;
        let ws = test_workspace("WS");
        store.create_workspace(&ws).await.unwrap();
        let res = test_resource(ws.id);
        store.create_resource(&res).await.unwrap();

        let retrieved = store.get_resource(res.id).await.unwrap().unwrap();
        assert_eq!(retrieved.name, "API Contract");
        assert_eq!(retrieved.resource_type, ResourceType::ApiContract);

        // List
        let resources = store.list_workspace_resources(ws.id).await.unwrap();
        assert_eq!(resources.len(), 1);

        // Update
        store
            .update_resource(
                res.id,
                Some("Updated Contract".to_string()),
                None,
                None,
                Some("2.0.0".to_string()),
                None,
            )
            .await
            .unwrap();
        let updated = store.get_resource(res.id).await.unwrap().unwrap();
        assert_eq!(updated.name, "Updated Contract");
        assert_eq!(updated.version, Some("2.0.0".to_string()));

        // Delete
        store.delete_resource(res.id).await.unwrap();
        assert!(store.get_resource(res.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_resource_project_links() {
        let store = setup().await;
        let ws = test_workspace("WS");
        store.create_workspace(&ws).await.unwrap();
        let res = test_resource(ws.id);
        store.create_resource(&res).await.unwrap();

        let p1 = test_project_named("Provider");
        let p2 = test_project_named("Consumer");
        store.create_project(&p1).await.unwrap();
        store.create_project(&p2).await.unwrap();

        store
            .link_project_implements_resource(p1.id, res.id)
            .await
            .unwrap();
        store
            .link_project_uses_resource(p2.id, res.id)
            .await
            .unwrap();

        let implementers = store.get_resource_implementers(res.id).await.unwrap();
        assert_eq!(implementers.len(), 1);
        assert_eq!(implementers[0].id, p1.id);

        let consumers = store.get_resource_consumers(res.id).await.unwrap();
        assert_eq!(consumers.len(), 1);
        assert_eq!(consumers[0].id, p2.id);
    }

    // =========================================================================
    // Component tests
    // =========================================================================

    #[tokio::test]
    async fn test_component_crud() {
        let store = setup().await;
        let ws = test_workspace("WS");
        store.create_workspace(&ws).await.unwrap();
        let comp = test_component(ws.id, "api-server");
        store.create_component(&comp).await.unwrap();

        let retrieved = store.get_component(comp.id).await.unwrap().unwrap();
        assert_eq!(retrieved.name, "api-server");
        assert_eq!(retrieved.component_type, ComponentType::Service);

        // List
        let components = store.list_components(ws.id).await.unwrap();
        assert_eq!(components.len(), 1);

        // Update
        store
            .update_component(
                comp.id,
                Some("new-api-server".to_string()),
                None,
                None,
                None,
                Some(vec!["updated".to_string()]),
            )
            .await
            .unwrap();
        let updated = store.get_component(comp.id).await.unwrap().unwrap();
        assert_eq!(updated.name, "new-api-server");
        assert_eq!(updated.tags, vec!["updated".to_string()]);

        // Delete
        store.delete_component(comp.id).await.unwrap();
        assert!(store.get_component(comp.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_component_dependencies_and_topology() {
        let store = setup().await;
        let ws = test_workspace("WS");
        store.create_workspace(&ws).await.unwrap();
        let api = test_component(ws.id, "api");
        let db = ComponentNode {
            component_type: ComponentType::Database,
            ..test_component(ws.id, "database")
        };
        store.create_component(&api).await.unwrap();
        store.create_component(&db).await.unwrap();

        store
            .add_component_dependency(api.id, db.id, Some("tcp".to_string()), true)
            .await
            .unwrap();

        let topology = store.get_workspace_topology(ws.id).await.unwrap();
        assert_eq!(topology.len(), 2);

        // Find the api component in topology
        let api_entry = topology.iter().find(|(c, _, _)| c.name == "api").unwrap();
        assert_eq!(api_entry.2.len(), 1);
        assert_eq!(api_entry.2[0].to_id, db.id);
        assert_eq!(api_entry.2[0].protocol, Some("tcp".to_string()));
        assert!(api_entry.2[0].required);

        // Remove dependency
        store
            .remove_component_dependency(api.id, db.id)
            .await
            .unwrap();
        let topology2 = store.get_workspace_topology(ws.id).await.unwrap();
        let api_entry2 = topology2.iter().find(|(c, _, _)| c.name == "api").unwrap();
        assert!(api_entry2.2.is_empty());
    }

    #[tokio::test]
    async fn test_map_component_to_project() {
        let store = setup().await;
        let ws = test_workspace("WS");
        store.create_workspace(&ws).await.unwrap();
        let comp = test_component(ws.id, "backend");
        store.create_component(&comp).await.unwrap();
        let project = test_project();
        store.create_project(&project).await.unwrap();

        store
            .map_component_to_project(comp.id, project.id)
            .await
            .unwrap();

        let topology = store.get_workspace_topology(ws.id).await.unwrap();
        let entry = topology
            .iter()
            .find(|(c, _, _)| c.name == "backend")
            .unwrap();
        assert_eq!(entry.1, Some(project.slug.clone()));
    }
}
