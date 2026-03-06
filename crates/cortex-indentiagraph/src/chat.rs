//! Chat session and event operations for IndentiaGraphStore.
//!
//! Implements chat session CRUD, event persistence/replay, and
//! DISCUSSED relation tracking between sessions and code entities.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{ChatEventRecord, ChatSessionNode, DiscussedEntity};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

// ---------------------------------------------------------------------------
// Record types (module-level for SurrealValue derive)
// ---------------------------------------------------------------------------

#[derive(Debug, SurrealValue)]
struct ChatSessionRecord {
    id: RecordId,
    cli_session_id: Option<String>,
    project_slug: Option<String>,
    workspace_slug: Option<String>,
    cwd: String,
    title: Option<String>,
    model: String,
    created_at: String,
    updated_at: String,
    message_count: i64,
    total_cost_usd: Option<f64>,
    conversation_id: Option<String>,
    preview: Option<String>,
    permission_mode: Option<String>,
    add_dirs: Option<Vec<String>>,
    auto_continue: Option<bool>,
}

impl ChatSessionRecord {
    fn into_node(self) -> Result<ChatSessionNode> {
        Ok(ChatSessionNode {
            id: rid_to_uuid(&self.id)?,
            cli_session_id: self.cli_session_id,
            project_slug: self.project_slug,
            workspace_slug: self.workspace_slug,
            cwd: self.cwd,
            title: self.title,
            model: self.model,
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            updated_at: self
                .updated_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            message_count: self.message_count,
            total_cost_usd: self.total_cost_usd,
            conversation_id: self.conversation_id,
            preview: self.preview,
            permission_mode: self.permission_mode,
            add_dirs: self.add_dirs,
        })
    }
}

#[derive(Debug, SurrealValue)]
struct ChatEventRec {
    id: RecordId,
    session_id: String,
    seq: i64,
    event_type: String,
    data: String,
    created_at: String,
}

impl ChatEventRec {
    fn into_node(self) -> Result<ChatEventRecord> {
        Ok(ChatEventRecord {
            id: rid_to_uuid(&self.id)?,
            session_id: Uuid::parse_str(&self.session_id).unwrap_or_else(|_| Uuid::default()),
            seq: self.seq,
            event_type: self.event_type,
            data: self.data,
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
        })
    }
}

#[derive(Debug, SurrealValue)]
struct DiscussedRecord {
    out: RecordId,
    mention_count: Option<i64>,
    last_mentioned_at: Option<String>,
}

// ===========================================================================
// Chat Session CRUD
// ===========================================================================

impl IndentiaGraphStore {
    pub async fn create_chat_session(&self, session: &ChatSessionNode) -> Result<()> {
        let rid = RecordId::new("chat_session", session.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 cli_session_id = $cli_id, project_slug = $pslug, \
                 workspace_slug = $wslug, cwd = $cwd, \
                 title = $title, model = $model, \
                 created_at = $created_at, updated_at = $updated_at, \
                 message_count = $msg_count, total_cost_usd = $cost, \
                 conversation_id = $conv_id, preview = $preview, \
                 permission_mode = $perm_mode, add_dirs = $add_dirs \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("cli_id", session.cli_session_id.clone()))
            .bind(("pslug", session.project_slug.clone()))
            .bind(("wslug", session.workspace_slug.clone()))
            .bind(("cwd", session.cwd.clone()))
            .bind(("title", session.title.clone()))
            .bind(("model", session.model.clone()))
            .bind(("created_at", session.created_at.to_rfc3339()))
            .bind(("updated_at", session.updated_at.to_rfc3339()))
            .bind(("msg_count", session.message_count))
            .bind(("cost", session.total_cost_usd))
            .bind(("conv_id", session.conversation_id.clone()))
            .bind(("preview", session.preview.clone()))
            .bind(("perm_mode", session.permission_mode.clone()))
            .bind(("add_dirs", session.add_dirs.clone()))
            .await
            .context("Failed to create chat session")?;
        Ok(())
    }

    pub async fn get_chat_session(&self, id: Uuid) -> Result<Option<ChatSessionNode>> {
        let rid = RecordId::new("chat_session", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get chat session")?;
        let records: Vec<ChatSessionRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn list_chat_sessions(
        &self,
        project_slug: Option<&str>,
        workspace_slug: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<ChatSessionNode>, usize)> {
        let mut conditions = Vec::new();
        if let Some(ps) = project_slug {
            conditions.push(format!("project_slug = '{}'", ps));
        }
        if let Some(ws) = workspace_slug {
            conditions.push(format!("workspace_slug = '{}'", ws));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let count_q = format!(
            "SELECT count() AS total FROM chat_session {} GROUP ALL",
            where_clause
        );
        let data_q = format!(
            "SELECT * FROM chat_session {} ORDER BY updated_at DESC LIMIT {} START {}",
            where_clause, limit, offset
        );

        let mut resp = self
            .db
            .query(format!("{}; {}", count_q, data_q))
            .await
            .context("Failed to list chat sessions")?;
        let count_result: Vec<serde_json::Value> = resp.take(0)?;
        let total = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let records: Vec<ChatSessionRecord> = resp.take(1)?;
        let sessions = records
            .into_iter()
            .filter_map(|r| r.into_node().ok())
            .collect();
        Ok((sessions, total))
    }

    pub async fn update_chat_session(
        &self,
        id: Uuid,
        cli_session_id: Option<String>,
        title: Option<String>,
        message_count: Option<i64>,
        total_cost_usd: Option<f64>,
        conversation_id: Option<String>,
        preview: Option<String>,
    ) -> Result<Option<ChatSessionNode>> {
        let mut sets = vec!["updated_at = $now".to_string()];
        if cli_session_id.is_some() {
            sets.push("cli_session_id = $cli_id".to_string());
        }
        if title.is_some() {
            sets.push("title = $title".to_string());
        }
        if message_count.is_some() {
            sets.push("message_count = $msg_count".to_string());
        }
        if total_cost_usd.is_some() {
            sets.push("total_cost_usd = $cost".to_string());
        }
        if conversation_id.is_some() {
            sets.push("conversation_id = $conv_id".to_string());
        }
        if preview.is_some() {
            sets.push("preview = $preview".to_string());
        }

        let rid = RecordId::new("chat_session", id.to_string().as_str());
        let query = format!("UPDATE $rid SET {} RETURN NONE", sets.join(", "));
        let mut q = self.db.query(&query);
        q = q.bind(("rid", rid)).bind(("now", Utc::now().to_rfc3339()));
        if let Some(ref cli) = cli_session_id {
            q = q.bind(("cli_id", cli.clone()));
        }
        if let Some(ref t) = title {
            q = q.bind(("title", t.clone()));
        }
        if let Some(mc) = message_count {
            q = q.bind(("msg_count", mc));
        }
        if let Some(cost) = total_cost_usd {
            q = q.bind(("cost", cost));
        }
        if let Some(ref cid) = conversation_id {
            q = q.bind(("conv_id", cid.clone()));
        }
        if let Some(ref p) = preview {
            q = q.bind(("preview", p.clone()));
        }
        q.await.context("Failed to update chat session")?;

        self.get_chat_session(id).await
    }

    pub async fn update_chat_session_permission_mode(&self, id: Uuid, mode: &str) -> Result<()> {
        let rid = RecordId::new("chat_session", id.to_string().as_str());
        self.db
            .query("UPDATE $rid SET permission_mode = $mode, updated_at = $now RETURN NONE")
            .bind(("rid", rid))
            .bind(("mode", mode.to_string()))
            .bind(("now", Utc::now().to_rfc3339()))
            .await
            .context("Failed to update chat session permission mode")?;
        Ok(())
    }

    pub async fn set_session_auto_continue(&self, id: Uuid, enabled: bool) -> Result<()> {
        let rid = RecordId::new("chat_session", id.to_string().as_str());
        self.db
            .query("UPDATE $rid SET auto_continue = $enabled, updated_at = $now RETURN NONE")
            .bind(("rid", rid))
            .bind(("enabled", enabled))
            .bind(("now", Utc::now().to_rfc3339()))
            .await
            .context("Failed to set session auto_continue")?;
        Ok(())
    }

    pub async fn get_session_auto_continue(&self, id: Uuid) -> Result<bool> {
        let rid = RecordId::new("chat_session", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT VALUE auto_continue FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get session auto_continue")?;
        let values: Vec<Option<bool>> = resp.take(0)?;
        Ok(values.into_iter().next().flatten().unwrap_or(false))
    }

    pub async fn backfill_chat_session_previews(&self) -> Result<usize> {
        // Find sessions without previews and try to populate from first event
        let mut resp = self
            .db
            .query(
                "SELECT id FROM chat_session WHERE preview IS NONE OR preview = '' \
                 ORDER BY created_at DESC LIMIT 100",
            )
            .await
            .context("Failed to query sessions without previews")?;
        let session_rids: Vec<serde_json::Value> = resp.take(0)?;

        let mut count = 0usize;
        for val in session_rids {
            let id_str = val.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            // Parse session ID from the record ID string
            let raw = id_str
                .trim_start_matches("chat_session:")
                .trim_start_matches('⟨')
                .trim_end_matches('⟩');
            let session_id = match Uuid::parse_str(raw) {
                Ok(u) => u,
                Err(_) => continue,
            };

            // Get first user_message event
            let sid = session_id.to_string();
            let mut evt_resp = self
                .db
                .query(
                    "SELECT data FROM chat_event \
                     WHERE session_id = $sid AND event_type = 'user_message' \
                     ORDER BY seq ASC LIMIT 1",
                )
                .bind(("sid", sid))
                .await?;
            let events: Vec<serde_json::Value> = evt_resp.take(0)?;
            if let Some(evt) = events.first() {
                let data_str = evt.get("data").and_then(|v| v.as_str()).unwrap_or_default();
                // Try to extract content from the data JSON
                let preview =
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data_str) {
                        parsed
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or(data_str)
                            .chars()
                            .take(200)
                            .collect::<String>()
                    } else {
                        data_str.chars().take(200).collect::<String>()
                    };

                if !preview.is_empty() {
                    let rid = RecordId::new("chat_session", session_id.to_string().as_str());
                    self.db
                        .query("UPDATE $rid SET preview = $preview RETURN NONE")
                        .bind(("rid", rid))
                        .bind(("preview", preview))
                        .await?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    pub async fn delete_chat_session(&self, id: Uuid) -> Result<bool> {
        // Check existence first
        let exists = self.get_chat_session(id).await?.is_some();
        if !exists {
            return Ok(false);
        }

        let sid = id.to_string();
        let rid = RecordId::new("chat_session", sid.as_str());

        // Delete events, discussed relations, and session
        self.db
            .query(
                "DELETE FROM chat_event WHERE session_id = $sid; \
                 DELETE FROM discussed WHERE in = type::record('chat_session', $sid); \
                 DELETE $rid",
            )
            .bind(("sid", sid))
            .bind(("rid", rid))
            .await
            .context("Failed to delete chat session")?;
        Ok(true)
    }

    // =========================================================================
    // Chat Events
    // =========================================================================

    pub async fn store_chat_events(
        &self,
        session_id: Uuid,
        events: Vec<ChatEventRecord>,
    ) -> Result<()> {
        for event in &events {
            let rid = RecordId::new("chat_event", event.id.to_string().as_str());
            self.db
                .query(
                    "CREATE $rid SET \
                     session_id = $sid, seq = $seq, \
                     event_type = $etype, data = $data, \
                     created_at = $created_at \
                     RETURN NONE",
                )
                .bind(("rid", rid))
                .bind(("sid", session_id.to_string()))
                .bind(("seq", event.seq))
                .bind(("etype", event.event_type.clone()))
                .bind(("data", event.data.clone()))
                .bind(("created_at", event.created_at.to_rfc3339()))
                .await
                .context("Failed to store chat event")?;
        }
        Ok(())
    }

    pub async fn get_chat_events(
        &self,
        session_id: Uuid,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<ChatEventRecord>> {
        let sid = session_id.to_string();
        let query = format!(
            "SELECT * FROM chat_event WHERE session_id = '{}' AND seq > {} ORDER BY seq ASC LIMIT {}",
            sid, after_seq, limit
        );
        let mut resp = self
            .db
            .query(&query)
            .await
            .context("Failed to get chat events")?;
        let records: Vec<ChatEventRec> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn get_chat_events_paginated(
        &self,
        session_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<ChatEventRecord>> {
        let sid = session_id.to_string();
        let query = format!(
            "SELECT * FROM chat_event WHERE session_id = '{}' ORDER BY seq ASC LIMIT {} START {}",
            sid, limit, offset
        );
        let mut resp = self
            .db
            .query(&query)
            .await
            .context("Failed to get chat events paginated")?;
        let records: Vec<ChatEventRec> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    pub async fn count_chat_events(&self, session_id: Uuid) -> Result<i64> {
        let sid = session_id.to_string();
        let query = format!(
            "SELECT count() AS total FROM chat_event WHERE session_id = '{}' GROUP ALL",
            sid
        );
        let mut resp = self.db.query(&query).await?;
        let result: Vec<serde_json::Value> = resp.take(0)?;
        let total = result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok(total)
    }

    pub async fn get_latest_chat_event_seq(&self, session_id: Uuid) -> Result<i64> {
        let sid = session_id.to_string();
        let query = format!(
            "SELECT VALUE seq FROM chat_event WHERE session_id = '{}' ORDER BY seq DESC LIMIT 1",
            sid
        );
        let mut resp = self.db.query(&query).await?;
        let seqs: Vec<i64> = resp.take(0)?;
        Ok(seqs.into_iter().next().unwrap_or(0))
    }

    pub async fn delete_chat_events(&self, session_id: Uuid) -> Result<()> {
        let sid = session_id.to_string();
        let query = format!("DELETE FROM chat_event WHERE session_id = '{}'", sid);
        self.db
            .query(&query)
            .await
            .context("Failed to delete chat events")?;
        Ok(())
    }

    // =========================================================================
    // Discussed entities
    // =========================================================================

    pub async fn add_discussed(
        &self,
        session_id: Uuid,
        entities: &[(String, String)],
    ) -> Result<usize> {
        let mut count = 0usize;
        let session_rid = RecordId::new("chat_session", session_id.to_string().as_str());

        for (entity_type, entity_id) in entities {
            let table = match entity_type.to_lowercase().as_str() {
                "file" => "file",
                "function" => "function",
                "struct" => "struct",
                "trait" => "trait",
                "enum" => "enum",
                _ => continue,
            };

            let lookup_field = if table == "file" { "path" } else { "name" };
            let query = format!(
                "LET $target = (SELECT id FROM {} WHERE {} = $eid LIMIT 1); \
                 IF $target[0] != NONE THEN \
                   (RELATE $from->discussed->$target[0].id \
                    SET mention_count = 1, last_mentioned_at = $now \
                    RETURN NONE) \
                 END",
                table, lookup_field
            );
            let result = self
                .db
                .query(&query)
                .bind(("from", session_rid.clone()))
                .bind(("eid", entity_id.clone()))
                .bind(("now", Utc::now().to_rfc3339()))
                .await;
            if result.is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }

    pub async fn get_session_entities(
        &self,
        session_id: Uuid,
        _project_id: Option<Uuid>,
    ) -> Result<Vec<DiscussedEntity>> {
        let sid = session_id.to_string();
        let mut resp = self
            .db
            .query(
                "SELECT out, mention_count, last_mentioned_at FROM discussed \
                 WHERE in = type::record('chat_session', $sid)",
            )
            .bind(("sid", sid))
            .await
            .context("Failed to get session entities")?;
        let records: Vec<DiscussedRecord> = resp.take(0)?;

        let mut entities = Vec::new();
        for rec in records {
            let out_table = rec.out.table.to_string();
            let out_id_str = match &rec.out.key {
                surrealdb::types::RecordIdKey::String(s) => s.clone(),
                surrealdb::types::RecordIdKey::Uuid(u) => u.to_string(),
                other => format!("{:?}", other),
            };
            let raw = out_id_str.trim_start_matches('⟨').trim_end_matches('⟩');

            let entity_type = match out_table.as_str() {
                "file" => "File",
                "function" => "Function",
                "struct" => "Struct",
                "trait" => "Trait",
                "enum" => "Enum",
                _ => "Unknown",
            };

            entities.push(DiscussedEntity {
                entity_type: entity_type.to_string(),
                entity_id: raw.to_string(),
                mention_count: rec.mention_count.unwrap_or(1),
                last_mentioned_at: rec.last_mentioned_at,
                file_path: None,
            });
        }
        Ok(entities)
    }

    pub async fn backfill_discussed(&self) -> Result<(usize, usize, usize)> {
        // Scan chat events for file/function mentions and create DISCUSSED relations
        // This is a best-effort backfill — returns (sessions_scanned, entities_found, relations_created)
        let mut resp = self
            .db
            .query("SELECT id, session_id FROM chat_event WHERE event_type = 'tool_use' OR event_type = 'tool_result' ORDER BY created_at DESC LIMIT 500")
            .await?;
        let events: Vec<serde_json::Value> = resp.take(0)?;
        let sessions_scanned = events.len();
        // Backfill is complex — return counts for now
        Ok((sessions_scanned, 0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    fn test_session(project_slug: Option<&str>) -> ChatSessionNode {
        ChatSessionNode {
            id: Uuid::new_v4(),
            cli_session_id: Some("cli-123".to_string()),
            project_slug: project_slug.map(|s| s.to_string()),
            workspace_slug: None,
            cwd: "/tmp/project".to_string(),
            title: Some("Test session".to_string()),
            model: "claude-opus-4-20250514".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            message_count: 0,
            total_cost_usd: None,
            conversation_id: None,
            preview: None,
            permission_mode: None,
            add_dirs: None,
        }
    }

    fn test_event(session_id: Uuid, seq: i64, event_type: &str) -> ChatEventRecord {
        ChatEventRecord {
            id: Uuid::new_v4(),
            session_id,
            seq,
            event_type: event_type.to_string(),
            data: serde_json::json!({"content": "Hello world"}).to_string(),
            created_at: Utc::now(),
        }
    }

    // =========================================================================
    // Session CRUD tests
    // =========================================================================

    #[tokio::test]
    async fn test_create_and_get_session() {
        let store = setup().await;
        let session = test_session(Some("test-project"));
        store.create_chat_session(&session).await.unwrap();

        let retrieved = store.get_chat_session(session.id).await.unwrap().unwrap();
        assert_eq!(retrieved.model, "claude-opus-4-20250514");
        assert_eq!(retrieved.project_slug, Some("test-project".to_string()));
        assert_eq!(retrieved.cli_session_id, Some("cli-123".to_string()));
    }

    #[tokio::test]
    async fn test_list_sessions_with_filter() {
        let store = setup().await;
        let s1 = test_session(Some("project-a"));
        let s2 = test_session(Some("project-b"));
        let s3 = test_session(Some("project-a"));
        store.create_chat_session(&s1).await.unwrap();
        store.create_chat_session(&s2).await.unwrap();
        store.create_chat_session(&s3).await.unwrap();

        let (all, total) = store.list_chat_sessions(None, None, 100, 0).await.unwrap();
        assert_eq!(total, 3);
        assert_eq!(all.len(), 3);

        let (filtered, ftotal) = store
            .list_chat_sessions(Some("project-a"), None, 100, 0)
            .await
            .unwrap();
        assert_eq!(ftotal, 2);
        assert_eq!(filtered.len(), 2);
    }

    #[tokio::test]
    async fn test_update_session() {
        let store = setup().await;
        let session = test_session(None);
        store.create_chat_session(&session).await.unwrap();

        let updated = store
            .update_chat_session(
                session.id,
                None,
                Some("New Title".to_string()),
                Some(5),
                Some(0.05),
                None,
                Some("How do I...".to_string()),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.title, Some("New Title".to_string()));
        assert_eq!(updated.message_count, 5);
        assert_eq!(updated.preview, Some("How do I...".to_string()));
    }

    #[tokio::test]
    async fn test_permission_mode() {
        let store = setup().await;
        let session = test_session(None);
        store.create_chat_session(&session).await.unwrap();

        store
            .update_chat_session_permission_mode(session.id, "auto-accept")
            .await
            .unwrap();

        let retrieved = store.get_chat_session(session.id).await.unwrap().unwrap();
        assert_eq!(retrieved.permission_mode, Some("auto-accept".to_string()));
    }

    #[tokio::test]
    async fn test_auto_continue() {
        let store = setup().await;
        let session = test_session(None);
        store.create_chat_session(&session).await.unwrap();

        assert!(!store.get_session_auto_continue(session.id).await.unwrap());

        store
            .set_session_auto_continue(session.id, true)
            .await
            .unwrap();
        assert!(store.get_session_auto_continue(session.id).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_session() {
        let store = setup().await;
        let session = test_session(None);
        store.create_chat_session(&session).await.unwrap();

        let deleted = store.delete_chat_session(session.id).await.unwrap();
        assert!(deleted);

        assert!(store.get_chat_session(session.id).await.unwrap().is_none());

        // Deleting non-existent returns false
        let deleted2 = store.delete_chat_session(Uuid::new_v4()).await.unwrap();
        assert!(!deleted2);
    }

    // =========================================================================
    // Event tests
    // =========================================================================

    #[tokio::test]
    async fn test_store_and_get_events() {
        let store = setup().await;
        let session = test_session(None);
        store.create_chat_session(&session).await.unwrap();

        let events = vec![
            test_event(session.id, 1, "user_message"),
            test_event(session.id, 2, "assistant_text"),
            test_event(session.id, 3, "tool_use"),
        ];
        store.store_chat_events(session.id, events).await.unwrap();

        // Get all events after seq 0
        let retrieved = store.get_chat_events(session.id, 0, 100).await.unwrap();
        assert_eq!(retrieved.len(), 3);
        assert_eq!(retrieved[0].seq, 1);
        assert_eq!(retrieved[2].seq, 3);

        // Get events after seq 1
        let partial = store.get_chat_events(session.id, 1, 100).await.unwrap();
        assert_eq!(partial.len(), 2);
    }

    #[tokio::test]
    async fn test_event_pagination() {
        let store = setup().await;
        let session = test_session(None);
        store.create_chat_session(&session).await.unwrap();

        let events: Vec<ChatEventRecord> = (1..=5)
            .map(|i| test_event(session.id, i, "assistant_text"))
            .collect();
        store.store_chat_events(session.id, events).await.unwrap();

        let page1 = store
            .get_chat_events_paginated(session.id, 0, 2)
            .await
            .unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].seq, 1);

        let page2 = store
            .get_chat_events_paginated(session.id, 2, 2)
            .await
            .unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].seq, 3);
    }

    #[tokio::test]
    async fn test_count_and_latest_seq() {
        let store = setup().await;
        let session = test_session(None);
        store.create_chat_session(&session).await.unwrap();

        assert_eq!(store.count_chat_events(session.id).await.unwrap(), 0);
        assert_eq!(
            store.get_latest_chat_event_seq(session.id).await.unwrap(),
            0
        );

        let events = vec![
            test_event(session.id, 1, "user_message"),
            test_event(session.id, 2, "assistant_text"),
        ];
        store.store_chat_events(session.id, events).await.unwrap();

        assert_eq!(store.count_chat_events(session.id).await.unwrap(), 2);
        assert_eq!(
            store.get_latest_chat_event_seq(session.id).await.unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn test_delete_events() {
        let store = setup().await;
        let session = test_session(None);
        store.create_chat_session(&session).await.unwrap();

        let events = vec![
            test_event(session.id, 1, "user_message"),
            test_event(session.id, 2, "assistant_text"),
        ];
        store.store_chat_events(session.id, events).await.unwrap();

        store.delete_chat_events(session.id).await.unwrap();
        assert_eq!(store.count_chat_events(session.id).await.unwrap(), 0);
    }
}
