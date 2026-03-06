//! File CRUD operations for IndentiaGraphStore.

use crate::client::IndentiaGraphStore;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{FileImportNode, FileNode, FileSymbolNamesNode};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

/// Helper: create a deterministic record ID for a file from its path.
fn file_record_id(path: &str) -> RecordId {
    RecordId::new("file", path)
}

fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| s.parse::<DateTime<Utc>>().ok())
}

/// SurrealDB record for files.
#[derive(Debug, SurrealValue)]
struct FileRecord {
    id: RecordId,
    path: String,
    language: String,
    hash: String,
    last_parsed: Option<String>,
    project_id: Option<String>,
}

impl FileRecord {
    fn into_file_node(self) -> FileNode {
        FileNode {
            path: self.path,
            language: self.language,
            hash: self.hash,
            last_parsed: self
                .last_parsed
                .as_deref()
                .and_then(parse_datetime)
                .unwrap_or_else(Utc::now),
            project_id: self
                .project_id
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok()),
        }
    }
}

/// SurrealDB record for counting.
#[derive(Debug, SurrealValue)]
struct CountRecord {
    count: i64,
}

/// SurrealDB record for path-only queries.
#[derive(Debug, SurrealValue)]
struct PathRecord {
    id: RecordId,
    path: String,
}

/// SurrealDB record for language queries.
#[derive(Debug, SurrealValue)]
struct LanguageRecord {
    language: String,
}

/// SurrealDB record for import path list.
#[derive(Debug, SurrealValue)]
struct ImportPathRecord {
    id: RecordId,
    path: String,
}

/// SurrealDB record for file imports (joined with language).
#[derive(Debug, SurrealValue)]
struct FileImportRecord {
    path: String,
    language: String,
}

/// SurrealDB record for symbol name aggregation.
#[derive(Debug, SurrealValue)]
struct SymbolNameRecord {
    name: String,
}

impl IndentiaGraphStore {
    // ========================================================================
    // File CRUD
    // ========================================================================

    /// Create or update a file node.
    pub async fn upsert_file(&self, file: &FileNode) -> Result<()> {
        let record_id = file_record_id(&file.path);
        self.db
            .query(
                "UPSERT $record_id SET \
                 path = $path, language = $language, hash = $hash, \
                 last_parsed = $last_parsed, project_id = $project_id \
                 RETURN NONE",
            )
            .bind(("record_id", record_id))
            .bind(("path", file.path.clone()))
            .bind(("language", file.language.clone()))
            .bind(("hash", file.hash.clone()))
            .bind(("last_parsed", file.last_parsed.to_rfc3339()))
            .bind(("project_id", file.project_id.map(|id| id.to_string())))
            .await
            .context("Failed to upsert file")?;
        Ok(())
    }

    /// Batch create or update file nodes.
    pub async fn batch_upsert_files(&self, files: &[FileNode]) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }

        // Process in chunks to avoid overly large queries
        for chunk in files.chunks(100) {
            let mut query_str = String::from("BEGIN TRANSACTION;\n");
            for (i, _file) in chunk.iter().enumerate() {
                query_str.push_str(&format!(
                    "UPSERT $rid_{i} SET \
                     path = $path_{i}, language = $lang_{i}, hash = $hash_{i}, \
                     last_parsed = $lp_{i}, project_id = $pid_{i} \
                     RETURN NONE;\n"
                ));
            }
            query_str.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query_str);
            for (i, file) in chunk.iter().enumerate() {
                q = q
                    .bind((format!("rid_{i}"), file_record_id(&file.path)))
                    .bind((format!("path_{i}"), file.path.clone()))
                    .bind((format!("lang_{i}"), file.language.clone()))
                    .bind((format!("hash_{i}"), file.hash.clone()))
                    .bind((format!("lp_{i}"), file.last_parsed.to_rfc3339()))
                    .bind((format!("pid_{i}"), file.project_id.map(|id| id.to_string())));
            }
            q.await.context("Failed to batch upsert files")?;
        }
        Ok(())
    }

    /// Get a file by path.
    pub async fn get_file(&self, path: &str) -> Result<Option<FileNode>> {
        let record_id = file_record_id(path);
        let mut response = self
            .db
            .query("SELECT * FROM $record_id")
            .bind(("record_id", record_id))
            .await
            .context("Failed to get file")?;

        let results: Vec<FileRecord> = response.take(0)?;
        Ok(results.into_iter().next().map(|r| r.into_file_node()))
    }

    /// List all files for a project.
    pub async fn list_project_files(&self, project_id: Uuid) -> Result<Vec<FileNode>> {
        let mut response = self
            .db
            .query("SELECT * FROM `file` WHERE project_id = $pid ORDER BY path ASC")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to list project files")?;

        let results: Vec<FileRecord> = response.take(0)?;
        Ok(results.into_iter().map(|r| r.into_file_node()).collect())
    }

    /// Count files for a project.
    pub async fn count_project_files(&self, project_id: Uuid) -> Result<i64> {
        let mut response = self
            .db
            .query("SELECT count() AS count FROM `file` WHERE project_id = $pid GROUP ALL")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to count project files")?;

        let results: Vec<CountRecord> = response.take(0)?;
        Ok(results.into_iter().next().map(|r| r.count).unwrap_or(0))
    }

    /// Get all file paths for a project.
    pub async fn get_project_file_paths(&self, project_id: Uuid) -> Result<Vec<String>> {
        let mut response = self
            .db
            .query("SELECT id, path FROM `file` WHERE project_id = $pid ORDER BY path ASC")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to get project file paths")?;

        let results: Vec<PathRecord> = response.take(0)?;
        Ok(results.into_iter().map(|r| r.path).collect())
    }

    /// Delete a file and all its contained symbols.
    pub async fn delete_file(&self, path: &str) -> Result<()> {
        let record_id = file_record_id(path);
        // Delete symbols contained by this file
        self.db
            .query(
                "DELETE `function` WHERE file_path = $path RETURN NONE;\
                 DELETE `struct` WHERE file_path = $path RETURN NONE;\
                 DELETE `trait` WHERE file_path = $path RETURN NONE;\
                 DELETE `enum` WHERE file_path = $path RETURN NONE;\
                 DELETE `impl` WHERE file_path = $path RETURN NONE;\
                 DELETE `import` WHERE file_path = $path RETURN NONE;\
                 DELETE $record_id RETURN NONE",
            )
            .bind(("path", path.to_string()))
            .bind(("record_id", record_id))
            .await
            .context("Failed to delete file")?;
        Ok(())
    }

    /// Delete stale files (those not in the valid_paths list).
    /// Returns (files_deleted, symbols_deleted, deleted_paths).
    pub async fn delete_stale_files(
        &self,
        project_id: Uuid,
        valid_paths: &[String],
    ) -> Result<(usize, usize, Vec<String>)> {
        // First find stale file paths
        let mut response = self
            .db
            .query("SELECT id, path FROM `file` WHERE project_id = $pid")
            .bind(("pid", project_id.to_string()))
            .await
            .context("Failed to query stale files")?;

        let all_files: Vec<PathRecord> = response.take(0)?;
        let stale_paths: Vec<String> = all_files
            .into_iter()
            .filter(|f| !valid_paths.contains(&f.path))
            .map(|f| f.path)
            .collect();

        if stale_paths.is_empty() {
            return Ok((0, 0, vec![]));
        }

        let files_deleted = stale_paths.len();
        let mut symbols_deleted = 0usize;

        for path in &stale_paths {
            // Count symbols before deleting
            let mut resp = self
                .db
                .query(
                    "SELECT count() AS count FROM `function` WHERE file_path = $path GROUP ALL;\
                     SELECT count() AS count FROM `struct` WHERE file_path = $path GROUP ALL;\
                     SELECT count() AS count FROM `trait` WHERE file_path = $path GROUP ALL;\
                     SELECT count() AS count FROM `enum` WHERE file_path = $path GROUP ALL;\
                     SELECT count() AS count FROM `impl` WHERE file_path = $path GROUP ALL;\
                     SELECT count() AS count FROM `import` WHERE file_path = $path GROUP ALL",
                )
                .bind(("path", path.clone()))
                .await
                .context("Failed to count symbols for stale file")?;

            for i in 0..6 {
                let counts: Vec<CountRecord> = resp.take(i)?;
                symbols_deleted += counts
                    .into_iter()
                    .next()
                    .map(|r| r.count as usize)
                    .unwrap_or(0);
            }

            self.delete_file(path).await?;
        }

        Ok((files_deleted, symbols_deleted, stale_paths))
    }

    /// Link a file to a project via CONTAINS edge.
    pub async fn link_file_to_project(&self, file_path: &str, project_id: Uuid) -> Result<()> {
        let proj_rid = RecordId::new("project", project_id.to_string().as_str());
        let file_rid = file_record_id(file_path);
        self.db
            .query(
                "UPSERT $file_rid SET project_id = $pid RETURN NONE;\
                 RELATE $proj_rid->contains->$file_rid RETURN NONE",
            )
            .bind(("file_rid", file_rid))
            .bind(("pid", project_id.to_string()))
            .bind(("proj_rid", proj_rid))
            .await
            .context("Failed to link file to project")?;
        Ok(())
    }

    /// Get the language of a file by path.
    pub async fn get_file_language(&self, path: &str) -> Result<Option<String>> {
        let record_id = file_record_id(path);
        let mut response = self
            .db
            .query("SELECT language FROM $record_id")
            .bind(("record_id", record_id))
            .await
            .context("Failed to get file language")?;

        let results: Vec<LanguageRecord> = response.take(0)?;
        Ok(results.into_iter().next().map(|r| r.language))
    }

    /// Get import paths for a file (from Import nodes).
    pub async fn get_file_import_paths_list(&self, path: &str) -> Result<Vec<String>> {
        let mut response = self
            .db
            .query("SELECT id, path FROM `import` WHERE file_path = $fp ORDER BY path ASC")
            .bind(("fp", path.to_string()))
            .await
            .context("Failed to get file import paths")?;

        let results: Vec<ImportPathRecord> = response.take(0)?;
        Ok(results.into_iter().map(|r| r.path).collect())
    }

    /// Get files directly imported by a file (via IMPORTS edges).
    pub async fn get_file_direct_imports(&self, path: &str) -> Result<Vec<FileImportNode>> {
        let mut response = self
            .db
            .query(
                "SELECT path, language FROM `file` WHERE id IN \
                 (SELECT VALUE out FROM `imports` WHERE in = type::record('file', $fp))",
            )
            .bind(("fp", path.to_string()))
            .await
            .context("Failed to get file direct imports")?;

        let results: Vec<FileImportRecord> = response.take(0)?;
        Ok(results
            .into_iter()
            .map(|r| FileImportNode {
                path: r.path,
                language: r.language,
            })
            .collect())
    }

    /// Get aggregated symbol names for a file.
    pub async fn get_file_symbol_names(&self, path: &str) -> Result<FileSymbolNamesNode> {
        let mut response = self
            .db
            .query(
                "SELECT name FROM `function` WHERE file_path = $fp ORDER BY name;\
                 SELECT name FROM `struct` WHERE file_path = $fp ORDER BY name;\
                 SELECT name FROM `trait` WHERE file_path = $fp ORDER BY name;\
                 SELECT name FROM `enum` WHERE file_path = $fp ORDER BY name",
            )
            .bind(("fp", path.to_string()))
            .await
            .context("Failed to get file symbol names")?;

        let funcs: Vec<SymbolNameRecord> = response.take(0)?;
        let structs: Vec<SymbolNameRecord> = response.take(1)?;
        let traits: Vec<SymbolNameRecord> = response.take(2)?;
        let enums: Vec<SymbolNameRecord> = response.take(3)?;

        Ok(FileSymbolNamesNode {
            functions: funcs.into_iter().map(|r| r.name).collect(),
            structs: structs.into_iter().map(|r| r.name).collect(),
            traits: traits.into_iter().map(|r| r.name).collect(),
            enums: enums.into_iter().map(|r| r.name).collect(),
        })
    }

    /// Invalidate pre-computed analytics properties on changed files.
    pub async fn invalidate_computed_properties(
        &self,
        _project_id: Uuid,
        paths: &[String],
    ) -> Result<u64> {
        if paths.is_empty() {
            return Ok(0);
        }
        let mut count = 0u64;
        for path in paths {
            let record_id = file_record_id(path);
            self.db
                .query(
                    "UPDATE $record_id SET \
                     cc_version = -1, structural_dna_version = -1, wl_hash_version = -1 \
                     RETURN NONE",
                )
                .bind(("record_id", record_id))
                .await
                .context("Failed to invalidate computed properties")?;
            count += 1;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_core::test_helpers::{test_file, test_file_for_project, test_project};

    async fn setup() -> IndentiaGraphStore {
        IndentiaGraphStore::new_memory().await.unwrap()
    }

    #[tokio::test]
    async fn test_upsert_and_get_file() {
        let store = setup().await;
        let file = test_file("/src/main.rs");

        store.upsert_file(&file).await.unwrap();

        let retrieved = store.get_file("/src/main.rs").await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.path, "/src/main.rs");
        assert_eq!(retrieved.language, "rust");
        assert_eq!(retrieved.hash, file.hash);
    }

    #[tokio::test]
    async fn test_get_nonexistent_file() {
        let store = setup().await;
        let result = store.get_file("/no/such/file.rs").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_upsert_file_updates_existing() {
        let store = setup().await;
        let mut file = test_file("/src/lib.rs");
        store.upsert_file(&file).await.unwrap();

        file.hash = "new_hash".to_string();
        file.language = "typescript".to_string();
        store.upsert_file(&file).await.unwrap();

        let retrieved = store.get_file("/src/lib.rs").await.unwrap().unwrap();
        assert_eq!(retrieved.hash, "new_hash");
        assert_eq!(retrieved.language, "typescript");
    }

    #[tokio::test]
    async fn test_batch_upsert_files() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let files: Vec<FileNode> = (0..10)
            .map(|i| test_file_for_project(&format!("/src/file_{}.rs", i), project.id))
            .collect();

        store.batch_upsert_files(&files).await.unwrap();

        let listed = store.list_project_files(project.id).await.unwrap();
        assert_eq!(listed.len(), 10);
    }

    #[tokio::test]
    async fn test_batch_upsert_files_empty() {
        let store = setup().await;
        store.batch_upsert_files(&[]).await.unwrap();
    }

    #[tokio::test]
    async fn test_list_project_files() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let f1 = test_file_for_project("/src/a.rs", project.id);
        let f2 = test_file_for_project("/src/b.rs", project.id);
        store.upsert_file(&f1).await.unwrap();
        store.upsert_file(&f2).await.unwrap();

        let files = store.list_project_files(project.id).await.unwrap();
        assert_eq!(files.len(), 2);
        // Should be sorted by path
        assert_eq!(files[0].path, "/src/a.rs");
        assert_eq!(files[1].path, "/src/b.rs");
    }

    #[tokio::test]
    async fn test_list_project_files_empty() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let files = store.list_project_files(project.id).await.unwrap();
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn test_count_project_files() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        assert_eq!(store.count_project_files(project.id).await.unwrap(), 0);

        for i in 0..5 {
            let f = test_file_for_project(&format!("/src/{}.rs", i), project.id);
            store.upsert_file(&f).await.unwrap();
        }

        assert_eq!(store.count_project_files(project.id).await.unwrap(), 5);
    }

    #[tokio::test]
    async fn test_get_project_file_paths() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let f1 = test_file_for_project("/src/alpha.rs", project.id);
        let f2 = test_file_for_project("/src/beta.rs", project.id);
        store.upsert_file(&f1).await.unwrap();
        store.upsert_file(&f2).await.unwrap();

        let paths = store.get_project_file_paths(project.id).await.unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"/src/alpha.rs".to_string()));
        assert!(paths.contains(&"/src/beta.rs".to_string()));
    }

    #[tokio::test]
    async fn test_delete_file() {
        let store = setup().await;
        let file = test_file("/src/delete_me.rs");
        store.upsert_file(&file).await.unwrap();

        assert!(store.get_file("/src/delete_me.rs").await.unwrap().is_some());

        store.delete_file("/src/delete_me.rs").await.unwrap();

        assert!(store.get_file("/src/delete_me.rs").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_delete_file_cascades_symbols() {
        let store = setup().await;
        let file = test_file("/src/symbols.rs");
        store.upsert_file(&file).await.unwrap();

        // Create a function in this file
        store
            .db
            .query(
                "CREATE `function` SET name = 'my_fn', file_path = '/src/symbols.rs', \
                 visibility = 'public', is_async = false, is_unsafe = false, \
                 complexity = 1, line_start = 1, line_end = 5 RETURN NONE",
            )
            .await
            .unwrap();

        store.delete_file("/src/symbols.rs").await.unwrap();

        // Verify function is also deleted
        let names = store
            .get_file_symbol_names("/src/symbols.rs")
            .await
            .unwrap();
        assert!(names.functions.is_empty());
    }

    #[tokio::test]
    async fn test_delete_stale_files() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let f1 = test_file_for_project("/src/keep.rs", project.id);
        let f2 = test_file_for_project("/src/stale1.rs", project.id);
        let f3 = test_file_for_project("/src/stale2.rs", project.id);
        store.upsert_file(&f1).await.unwrap();
        store.upsert_file(&f2).await.unwrap();
        store.upsert_file(&f3).await.unwrap();

        let valid = vec!["/src/keep.rs".to_string()];
        let (files_del, _syms_del, deleted_paths) =
            store.delete_stale_files(project.id, &valid).await.unwrap();

        assert_eq!(files_del, 2);
        assert!(deleted_paths.contains(&"/src/stale1.rs".to_string()));
        assert!(deleted_paths.contains(&"/src/stale2.rs".to_string()));

        // Verify kept file still exists
        assert!(store.get_file("/src/keep.rs").await.unwrap().is_some());
        assert!(store.get_file("/src/stale1.rs").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_link_file_to_project() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let file = test_file("/src/orphan.rs");
        store.upsert_file(&file).await.unwrap();

        // File initially has no project
        let retrieved = store.get_file("/src/orphan.rs").await.unwrap().unwrap();
        assert!(retrieved.project_id.is_none());

        store
            .link_file_to_project("/src/orphan.rs", project.id)
            .await
            .unwrap();

        let linked = store.get_file("/src/orphan.rs").await.unwrap().unwrap();
        assert_eq!(linked.project_id, Some(project.id));
    }

    #[tokio::test]
    async fn test_get_file_language() {
        let store = setup().await;
        let mut file = test_file("/src/app.tsx");
        file.language = "typescript".to_string();
        store.upsert_file(&file).await.unwrap();

        let lang = store.get_file_language("/src/app.tsx").await.unwrap();
        assert_eq!(lang, Some("typescript".to_string()));
    }

    #[tokio::test]
    async fn test_get_file_language_not_found() {
        let store = setup().await;
        let lang = store.get_file_language("/no/such.rs").await.unwrap();
        assert!(lang.is_none());
    }

    #[tokio::test]
    async fn test_get_file_symbol_names() {
        let store = setup().await;
        let file = test_file("/src/models.rs");
        store.upsert_file(&file).await.unwrap();

        // Create symbols in this file
        store
            .db
            .query(
                "CREATE `function` SET name = 'process', file_path = '/src/models.rs', \
                 visibility = 'public', is_async = false, is_unsafe = false, \
                 complexity = 1, line_start = 1, line_end = 5 RETURN NONE;\
                 CREATE `struct` SET name = 'User', file_path = '/src/models.rs', \
                 visibility = 'public', line_start = 10, line_end = 15 RETURN NONE;\
                 CREATE `trait` SET name = 'Handler', file_path = '/src/models.rs', \
                 visibility = 'public', line_start = 20, line_end = 25, \
                 is_external = false RETURN NONE;\
                 CREATE `enum` SET name = 'Status', file_path = '/src/models.rs', \
                 visibility = 'public', line_start = 30, line_end = 35 RETURN NONE",
            )
            .await
            .unwrap();

        let names = store.get_file_symbol_names("/src/models.rs").await.unwrap();
        assert_eq!(names.functions, vec!["process"]);
        assert_eq!(names.structs, vec!["User"]);
        assert_eq!(names.traits, vec!["Handler"]);
        assert_eq!(names.enums, vec!["Status"]);
    }

    #[tokio::test]
    async fn test_get_file_symbol_names_empty() {
        let store = setup().await;
        let names = store.get_file_symbol_names("/src/empty.rs").await.unwrap();
        assert!(names.functions.is_empty());
        assert!(names.structs.is_empty());
        assert!(names.traits.is_empty());
        assert!(names.enums.is_empty());
    }

    #[tokio::test]
    async fn test_invalidate_computed_properties() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let file = test_file_for_project("/src/compute.rs", project.id);
        store.upsert_file(&file).await.unwrap();

        let count = store
            .invalidate_computed_properties(project.id, &["/src/compute.rs".to_string()])
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_invalidate_computed_properties_empty() {
        let store = setup().await;
        let project = test_project();
        let count = store
            .invalidate_computed_properties(project.id, &[])
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_file_isolation_between_projects() {
        let store = setup().await;
        let p1 = cortex_core::test_helpers::test_project_named("proj-a");
        let p2 = cortex_core::test_helpers::test_project_named("proj-b");
        store.create_project(&p1).await.unwrap();
        store.create_project(&p2).await.unwrap();

        let f1 = test_file_for_project("/src/shared.rs", p1.id);
        let f2 = test_file_for_project("/src/other.rs", p2.id);
        store.upsert_file(&f1).await.unwrap();
        store.upsert_file(&f2).await.unwrap();

        let p1_files = store.list_project_files(p1.id).await.unwrap();
        let p2_files = store.list_project_files(p2.id).await.unwrap();
        assert_eq!(p1_files.len(), 1);
        assert_eq!(p2_files.len(), 1);
        assert_eq!(p1_files[0].path, "/src/shared.rs");
        assert_eq!(p2_files[0].path, "/src/other.rs");
    }

    #[tokio::test]
    async fn test_concurrent_file_upserts() {
        let store = setup().await;
        let store = std::sync::Arc::new(store);

        let mut handles = Vec::new();
        for i in 0..20 {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                let f = test_file(&format!("/src/concurrent_{}.rs", i));
                s.upsert_file(&f).await.unwrap();
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        // Verify all 20 files exist
        for i in 0..20 {
            let f = store
                .get_file(&format!("/src/concurrent_{}.rs", i))
                .await
                .unwrap();
            assert!(f.is_some(), "File {} should exist", i);
        }
    }
}
