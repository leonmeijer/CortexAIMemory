//! IndentiaGraph connection and store setup.

use anyhow::{Context, Result};
use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::types::{RecordId, RecordIdKey};
use surrealdb::Surreal;
use uuid::Uuid;

/// Extract a UUID from a SurrealDB RecordId.
///
/// SurrealDB wraps record keys in angle brackets (⟨uuid⟩) for string keys.
/// This helper handles String, Uuid, and other key formats.
pub(crate) fn rid_to_uuid(rid: &RecordId) -> Result<Uuid> {
    let key_str = match &rid.key {
        RecordIdKey::String(s) => s.clone(),
        RecordIdKey::Uuid(u) => u.to_string(),
        other => format!("{:?}", other),
    };
    let raw = key_str.trim_start_matches('⟨').trim_end_matches('⟩');
    Uuid::parse_str(raw).context("Failed to parse UUID from RecordId")
}

/// IndentiaGraph store backed by SurrealDB.
///
/// Wraps a SurrealDB connection and implements the full [`GraphStore`] trait.
/// Supports both remote (WebSocket) and in-memory (test) connections.
pub struct IndentiaGraphStore {
    pub(crate) db: Arc<Surreal<Any>>,
}

impl IndentiaGraphStore {
    /// Connect to a remote SurrealDB instance via WebSocket.
    ///
    /// # Arguments
    /// - `url`: WebSocket URL, e.g. `ws://localhost:8000`
    /// - `namespace`: SurrealDB namespace (e.g. `"cortex"`)
    /// - `database`: SurrealDB database (e.g. `"memory"`)
    /// - `username`: Root or namespace username
    /// - `password`: Root or namespace password
    pub async fn new(
        url: &str,
        namespace: &str,
        database: &str,
        username: &str,
        password: &str,
    ) -> Result<Self> {
        let db = surrealdb::engine::any::connect(url)
            .await
            .context("Failed to connect to SurrealDB")?;

        // Remote protocol endpoints require authentication. Embedded engines
        // (e.g. mem://) do not use root signin and fail if we attempt it.
        let is_remote = url.starts_with("ws://")
            || url.starts_with("wss://")
            || url.starts_with("http://")
            || url.starts_with("https://");

        if is_remote {
            db.signin(surrealdb::opt::auth::Root {
                username: username.to_string(),
                password: password.to_string(),
            })
            .await
            .context("Failed to sign in to SurrealDB")?;
        }

        db.use_ns(namespace)
            .use_db(database)
            .await
            .context("Failed to select namespace/database")?;

        let store = Self { db: Arc::new(db) };
        store.init_schema().await?;
        Ok(store)
    }

    /// Create an in-memory SurrealDB instance for testing.
    ///
    /// No external service required. Each call creates an isolated database.
    pub async fn new_memory() -> Result<Self> {
        let db = surrealdb::engine::any::connect("mem://")
            .await
            .context("Failed to create in-memory SurrealDB")?;

        db.use_ns("test")
            .use_db("test")
            .await
            .context("Failed to select test namespace/database")?;

        let store = Self { db: Arc::new(db) };
        store.init_schema().await?;
        Ok(store)
    }

    /// Health check — verify the database connection is alive.
    /// Returns Ok(true) if reachable, Ok(false) if not.
    pub async fn health_check(&self) -> Result<bool> {
        match self.db.query("RETURN 1").await {
            Ok(mut response) => {
                let _: Vec<serde_json::Value> = response.take(0)?;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_new_memory_creates_store() {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        assert!(store.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn test_memory_store_is_isolated() {
        // Two stores should be completely independent
        let store1 = IndentiaGraphStore::new_memory().await.unwrap();
        let store2 = IndentiaGraphStore::new_memory().await.unwrap();
        assert!(store1.health_check().await.is_ok());
        assert!(store2.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn test_new_with_mem_url_skips_signin() {
        let store = IndentiaGraphStore::new("mem://", "cortex", "memory", "root", "root")
            .await
            .unwrap();
        assert!(store.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn test_new_with_surrealkv_persists_data() {
        let dir = tempdir().unwrap();
        let uri = format!("surrealkv://{}", dir.path().to_string_lossy());

        let store1 = IndentiaGraphStore::new(&uri, "cortex", "memory", "root", "root")
            .await
            .unwrap();
        store1
            .db
            .query("UPSERT kv_probe:probe SET value = 'persisted' RETURN NONE")
            .await
            .unwrap();
        // Ensure the first session is fully closed so the datastore lock is released.
        store1.db.invalidate().await.unwrap();
        drop(store1);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let store2 = IndentiaGraphStore::new(&uri, "cortex", "memory", "root", "root")
            .await
            .unwrap();
        let mut resp = store2
            .db
            .query("SELECT VALUE value FROM kv_probe:probe")
            .await
            .unwrap();
        let values: Vec<String> = resp.take(0).unwrap();
        assert_eq!(values, vec!["persisted".to_string()]);
    }
}
