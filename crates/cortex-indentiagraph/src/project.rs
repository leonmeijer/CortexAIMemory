//! Project CRUD operations for IndentiaGraphStore.

use crate::client::IndentiaGraphStore;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::ProjectNode;
use surrealdb::types::{RecordId, RecordIdKey, SurrealValue};
use uuid::Uuid;

/// SurrealDB record for projects — used for deserialization from queries.
#[derive(Debug, SurrealValue)]
pub(crate) struct ProjectRecord {
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

fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| s.parse::<DateTime<Utc>>().ok())
}

impl ProjectRecord {
    pub(crate) fn into_project_node(self) -> Option<ProjectNode> {
        // Extract UUID from SurrealDB record ID key
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
                .and_then(parse_datetime)
                .unwrap_or_else(Utc::now),
            last_synced: self.last_synced.as_deref().and_then(parse_datetime),
            analytics_computed_at: self
                .analytics_computed_at
                .as_deref()
                .and_then(parse_datetime),
            last_co_change_computed_at: self
                .last_co_change_computed_at
                .as_deref()
                .and_then(parse_datetime),
        })
    }
}

impl IndentiaGraphStore {
    /// Create a new project.
    pub async fn create_project(&self, project: &ProjectNode) -> Result<()> {
        let record_id = RecordId::new("project", project.id.to_string().as_str());
        self.db
            .query(
                "CREATE $record_id SET \
                 name = $name, slug = $slug, root_path = $root_path, \
                 description = $description, created_at = $created_at, \
                 last_synced = $last_synced, analytics_computed_at = $analytics_computed_at, \
                 last_co_change_computed_at = $last_co_change_computed_at \
                 RETURN NONE",
            )
            .bind(("record_id", record_id))
            .bind(("name", project.name.clone()))
            .bind(("slug", project.slug.clone()))
            .bind(("root_path", project.root_path.clone()))
            .bind(("description", project.description.clone()))
            .bind(("created_at", project.created_at.to_rfc3339()))
            .bind(("last_synced", project.last_synced.map(|t| t.to_rfc3339())))
            .bind((
                "analytics_computed_at",
                project.analytics_computed_at.map(|t| t.to_rfc3339()),
            ))
            .bind((
                "last_co_change_computed_at",
                project.last_co_change_computed_at.map(|t| t.to_rfc3339()),
            ))
            .await
            .context("Failed to create project")?;
        Ok(())
    }

    /// Get a project by ID.
    pub async fn get_project(&self, id: Uuid) -> Result<Option<ProjectNode>> {
        let record_id = RecordId::new("project", id.to_string().as_str());
        let mut response = self
            .db
            .query("SELECT * FROM $record_id")
            .bind(("record_id", record_id))
            .await
            .context("Failed to get project")?;

        let results: Vec<ProjectRecord> = response.take(0)?;
        Ok(results
            .into_iter()
            .next()
            .and_then(|r| r.into_project_node()))
    }

    /// Get a project by slug.
    pub async fn get_project_by_slug(&self, slug: &str) -> Result<Option<ProjectNode>> {
        let mut response = self
            .db
            .query("SELECT * FROM project WHERE slug = $slug LIMIT 1")
            .bind(("slug", slug.to_string()))
            .await
            .context("Failed to get project by slug")?;

        let results: Vec<ProjectRecord> = response.take(0)?;
        Ok(results
            .into_iter()
            .next()
            .and_then(|r| r.into_project_node()))
    }

    /// List all projects.
    pub async fn list_projects(&self) -> Result<Vec<ProjectNode>> {
        let mut response = self
            .db
            .query("SELECT * FROM project ORDER BY created_at DESC")
            .await
            .context("Failed to list projects")?;

        let results: Vec<ProjectRecord> = response.take(0)?;
        Ok(results
            .into_iter()
            .filter_map(|r| r.into_project_node())
            .collect())
    }

    /// Update project fields.
    pub async fn update_project(
        &self,
        id: Uuid,
        name: Option<String>,
        description: Option<Option<String>>,
        root_path: Option<String>,
    ) -> Result<()> {
        let mut sets = Vec::new();
        if name.is_some() {
            sets.push("name = $name");
        }
        if description.is_some() {
            sets.push("description = $description");
        }
        if root_path.is_some() {
            sets.push("root_path = $root_path");
        }

        if sets.is_empty() {
            return Ok(());
        }

        let record_id = RecordId::new("project", id.to_string().as_str());
        let query = format!("UPDATE $record_id SET {} RETURN NONE", sets.join(", "));

        let mut q = self.db.query(&query).bind(("record_id", record_id));

        if let Some(n) = name {
            q = q.bind(("name", n));
        }
        if let Some(d) = description {
            q = q.bind(("description", d));
        }
        if let Some(r) = root_path {
            q = q.bind(("root_path", r));
        }

        q.await.context("Failed to update project")?;
        Ok(())
    }

    /// Update project last_synced timestamp.
    pub async fn update_project_synced(&self, id: Uuid) -> Result<()> {
        let record_id = RecordId::new("project", id.to_string().as_str());
        let now = Utc::now().to_rfc3339();
        self.db
            .query("UPDATE $record_id SET last_synced = $now RETURN NONE")
            .bind(("record_id", record_id))
            .bind(("now", now))
            .await
            .context("Failed to update project sync timestamp")?;
        Ok(())
    }

    /// Update project analytics_computed_at timestamp.
    pub async fn update_project_analytics_timestamp(&self, id: Uuid) -> Result<()> {
        let record_id = RecordId::new("project", id.to_string().as_str());
        let now = Utc::now().to_rfc3339();
        self.db
            .query("UPDATE $record_id SET analytics_computed_at = $now RETURN NONE")
            .bind(("record_id", record_id))
            .bind(("now", now))
            .await
            .context("Failed to update project analytics timestamp")?;
        Ok(())
    }

    /// Delete a project and all its related data.
    ///
    /// Deletion order:
    /// 1. Symbols (function, struct, trait, enum, impl, import) — via file_path
    /// 2. Files — have project_id
    /// 3. Plans → tasks → steps, decisions, constraints — via plan_id / task_id
    /// 4. Direct project_id tables: note, episode, release, milestone, skill, commit,
    ///    context_card, feature_graph, topology_rule, analysis_profile, process,
    ///    resource, component, chat_session (project_slug)
    /// 5. The project record itself
    pub async fn delete_project(&self, id: Uuid, _project_name: &str) -> Result<()> {
        let pid = id.to_string();
        let record_id = RecordId::new("project", pid.as_str());
        let project_slug = {
            let mut resp = self
                .db
                .query("SELECT VALUE slug FROM $record_id LIMIT 1")
                .bind(("record_id", record_id.clone()))
                .await
                .context("Failed to resolve project slug for cascade delete")?;
            let slugs: Vec<String> = resp.take(0).unwrap_or_default();
            slugs.into_iter().next()
        };

        // Step 1: Delete symbols linked to files belonging to this project.
        // Symbols don't have project_id directly — they reference file_path.
        self.db
            .query(
                "DELETE `function` WHERE file_path IN (SELECT VALUE path FROM `file` WHERE project_id = $pid) RETURN NONE;\
                 DELETE `struct`   WHERE file_path IN (SELECT VALUE path FROM `file` WHERE project_id = $pid) RETURN NONE;\
                 DELETE `trait`    WHERE file_path IN (SELECT VALUE path FROM `file` WHERE project_id = $pid) RETURN NONE;\
                 DELETE `enum`     WHERE file_path IN (SELECT VALUE path FROM `file` WHERE project_id = $pid) RETURN NONE;\
                 DELETE `impl`     WHERE file_path IN (SELECT VALUE path FROM `file` WHERE project_id = $pid) RETURN NONE;\
                 DELETE `import`   WHERE file_path IN (SELECT VALUE path FROM `file` WHERE project_id = $pid) RETURN NONE",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to cascade delete symbols for project")?;

        // Step 2: Delete files.
        self.db
            .query("DELETE `file` WHERE project_id = $pid RETURN NONE")
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to cascade delete files for project")?;

        // Step 3: Delete planning hierarchy — steps and decisions via task_id,
        //         constraints via plan_id, then tasks, then plans.
        self.db
            .query(
                "DELETE `step`       WHERE task_id IN (SELECT VALUE id FROM `task` WHERE plan_id IN (SELECT VALUE id FROM `plan` WHERE project_id = $pid)) RETURN NONE;\
                 DELETE `decision`   WHERE task_id IN (SELECT VALUE id FROM `task` WHERE plan_id IN (SELECT VALUE id FROM `plan` WHERE project_id = $pid)) RETURN NONE;\
                 DELETE `constraint` WHERE plan_id IN (SELECT VALUE id FROM `plan` WHERE project_id = $pid) RETURN NONE;\
                 DELETE `task`       WHERE plan_id IN (SELECT VALUE id FROM `plan` WHERE project_id = $pid) RETURN NONE;\
                 DELETE `plan`       WHERE project_id = $pid RETURN NONE",
            )
            .bind(("pid", pid.clone()))
            .await
            .context("Failed to cascade delete plans/tasks for project")?;

        // Step 4: Delete direct project_id tables.
        for table in &[
            "note",
            "episode",
            "release",
            "milestone",
            "skill",
            "commit",
            "context_card",
            "feature_graph",
            "topology_rule",
            "analysis_profile",
            "process",
            "resource",
            "component",
        ] {
            self.db
                .query(&format!(
                    "DELETE `{}` WHERE project_id = $pid RETURN NONE",
                    table
                ))
                .bind(("pid", pid.clone()))
                .await
                .context(format!("Failed to cascade delete {} for project", table))?;
        }

        // chat_session uses project_slug, not project_id.
        if let Some(slug) = project_slug {
            self.db
                .query("DELETE `chat_session` WHERE project_slug = $slug RETURN NONE")
                .bind(("slug", slug))
                .await
                .context("Failed to cascade delete chat_session for project")?;
        }

        // Step 5: Delete the project record itself.
        self.db
            .query("DELETE $record_id RETURN NONE")
            .bind(("record_id", record_id))
            .await
            .context("Failed to delete project")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_core::test_helpers::{test_project, test_project_named};

    async fn setup() -> IndentiaGraphStore {
        IndentiaGraphStore::new_memory().await.unwrap()
    }

    #[tokio::test]
    async fn test_create_and_get_project() {
        let store = setup().await;
        let project = test_project();

        store.create_project(&project).await.unwrap();

        let retrieved = store.get_project(project.id).await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, project.id);
        assert_eq!(retrieved.name, project.name);
        assert_eq!(retrieved.slug, project.slug);
        assert_eq!(retrieved.root_path, project.root_path);
        assert_eq!(retrieved.description, project.description);
    }

    #[tokio::test]
    async fn test_get_nonexistent_project() {
        let store = setup().await;
        let result = store.get_project(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_project_by_slug() {
        let store = setup().await;
        let project = test_project_named("My Cool Project");

        store.create_project(&project).await.unwrap();

        let retrieved = store.get_project_by_slug(&project.slug).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, project.id);
    }

    #[tokio::test]
    async fn test_get_project_by_slug_not_found() {
        let store = setup().await;
        let result = store.get_project_by_slug("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_projects_empty() {
        let store = setup().await;
        let projects = store.list_projects().await.unwrap();
        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn test_list_projects_multiple() {
        let store = setup().await;
        let p1 = test_project_named("Alpha");
        let p2 = test_project_named("Beta");
        let p3 = test_project_named("Gamma");

        store.create_project(&p1).await.unwrap();
        store.create_project(&p2).await.unwrap();
        store.create_project(&p3).await.unwrap();

        let projects = store.list_projects().await.unwrap();
        assert_eq!(projects.len(), 3);
    }

    #[tokio::test]
    async fn test_update_project_name() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        store
            .update_project(project.id, Some("New Name".to_string()), None, None)
            .await
            .unwrap();

        let updated = store.get_project(project.id).await.unwrap().unwrap();
        assert_eq!(updated.name, "New Name");
        assert_eq!(updated.slug, project.slug);
        assert_eq!(updated.root_path, project.root_path);
    }

    #[tokio::test]
    async fn test_update_project_description() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        store
            .update_project(
                project.id,
                None,
                Some(Some("Updated description".to_string())),
                None,
            )
            .await
            .unwrap();

        let updated = store.get_project(project.id).await.unwrap().unwrap();
        assert_eq!(updated.description, Some("Updated description".to_string()));

        store
            .update_project(project.id, None, Some(None), None)
            .await
            .unwrap();

        let cleared = store.get_project(project.id).await.unwrap().unwrap();
        assert!(cleared.description.is_none());
    }

    #[tokio::test]
    async fn test_update_project_noop() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        store
            .update_project(project.id, None, None, None)
            .await
            .unwrap();

        let same = store.get_project(project.id).await.unwrap().unwrap();
        assert_eq!(same.name, project.name);
    }

    #[tokio::test]
    async fn test_update_project_synced() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        assert!(project.last_synced.is_none());

        store.update_project_synced(project.id).await.unwrap();

        let updated = store.get_project(project.id).await.unwrap().unwrap();
        assert!(updated.last_synced.is_some());
    }

    #[tokio::test]
    async fn test_update_project_analytics_timestamp() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();
        assert!(project.analytics_computed_at.is_none());

        store
            .update_project_analytics_timestamp(project.id)
            .await
            .unwrap();

        let updated = store.get_project(project.id).await.unwrap().unwrap();
        assert!(updated.analytics_computed_at.is_some());
    }

    #[tokio::test]
    async fn test_delete_project() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        store
            .delete_project(project.id, &project.name)
            .await
            .unwrap();

        let deleted = store.get_project(project.id).await.unwrap();
        assert!(deleted.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_project() {
        let store = setup().await;
        store.delete_project(Uuid::new_v4(), "ghost").await.unwrap();
    }

    #[tokio::test]
    async fn test_project_round_trip_all_fields() {
        let store = setup().await;
        let mut project = test_project();
        project.last_synced = Some(Utc::now());
        project.analytics_computed_at = Some(Utc::now());
        project.last_co_change_computed_at = Some(Utc::now());

        store.create_project(&project).await.unwrap();

        let retrieved = store.get_project(project.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, project.id);
        assert_eq!(retrieved.name, project.name);
        assert!(retrieved.last_synced.is_some());
        assert!(retrieved.analytics_computed_at.is_some());
        assert!(retrieved.last_co_change_computed_at.is_some());
    }

    #[tokio::test]
    async fn test_concurrent_project_creation() {
        let store = setup().await;
        let store = std::sync::Arc::new(store);

        let mut handles = Vec::new();
        for i in 0..10 {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                let p = test_project_named(&format!("concurrent-{}", i));
                s.create_project(&p).await.unwrap();
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let projects = store.list_projects().await.unwrap();
        assert_eq!(projects.len(), 10);
    }
}
