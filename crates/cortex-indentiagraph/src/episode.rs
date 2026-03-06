//! Episode storage implementation for IndentiaGraph

use anyhow::Result;
use chrono::{DateTime, Utc};
use cortex_core::episode::{CreateEpisodeRequest, Episode, EpisodeSource};
use cortex_core::notes::Note;
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::IndentiaGraphStore;
use crate::note::json_value_to_note;

// ---------------------------------------------------------------------------
// Typed record structs (SurrealDB 3.x requires typed structs for SELECT)
// ---------------------------------------------------------------------------

/// Typed record for deserializing episode rows from SurrealDB.
#[derive(Debug, SurrealValue)]
struct EpisodeRecord {
    id: RecordId,
    name: String,
    content: String,
    source: String,
    reference_time: Option<String>,
    ingested_at: Option<String>,
    project_id: Option<String>,
    group_id: Option<String>,
}

impl EpisodeRecord {
    fn into_episode(self) -> Episode {
        use surrealdb::types::RecordIdKey;

        let source = self
            .source
            .parse::<EpisodeSource>()
            .unwrap_or(EpisodeSource::Event);

        let key_str = match &self.id.key {
            RecordIdKey::String(s) => s.trim_start_matches('⟨').trim_end_matches('⟩').to_string(),
            RecordIdKey::Uuid(u) => u.to_string(),
            other => format!("{:?}", other),
        };
        let id_str = format!("episode:{}", key_str);

        let reference_time = self
            .reference_time
            .as_deref()
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);

        let ingested_at = self
            .ingested_at
            .as_deref()
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);

        Episode {
            id: id_str,
            name: self.name,
            content: self.content,
            source,
            reference_time,
            ingested_at,
            project_id: self.project_id,
            group_id: self.group_id,
        }
    }
}

impl IndentiaGraphStore {
    pub async fn add_episode(&self, req: CreateEpisodeRequest) -> Result<Episode> {
        let id = Uuid::new_v4().to_string();
        let reference_time = req.reference_time.unwrap_or_else(Utc::now);
        let ingested_at = Utc::now();

        let rid = RecordId::new("episode", id.as_str());

        let sql = "CREATE $rid SET \
                   name = $name, \
                   content = $content, \
                   source = $source, \
                   reference_time = $reference_time, \
                   ingested_at = $ingested_at, \
                   project_id = $project_id, \
                   group_id = $group_id \
                   RETURN NONE";

        self.db
            .query(sql)
            .bind(("rid", rid))
            .bind(("name", req.name.clone()))
            .bind(("content", req.content.clone()))
            .bind(("source", req.source.to_string()))
            .bind(("reference_time", reference_time.to_rfc3339()))
            .bind(("ingested_at", ingested_at.to_rfc3339()))
            .bind(("project_id", req.project_id.clone()))
            .bind(("group_id", req.group_id.clone()))
            .await?;

        Ok(Episode {
            id: format!("episode:{id}"),
            name: req.name,
            content: req.content,
            source: req.source,
            reference_time,
            ingested_at,
            project_id: req.project_id,
            group_id: req.group_id,
        })
    }

    pub async fn get_episodes(
        &self,
        project_id: Option<&str>,
        group_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Episode>> {
        let sql = if project_id.is_some() {
            r#"SELECT * FROM episode WHERE project_id = $project_id ORDER BY reference_time DESC LIMIT $limit"#
        } else if group_id.is_some() {
            r#"SELECT * FROM episode WHERE group_id = $group_id ORDER BY reference_time DESC LIMIT $limit"#
        } else {
            r#"SELECT * FROM episode ORDER BY reference_time DESC LIMIT $limit"#
        };

        let mut query = self.db.query(sql).bind(("limit", limit as i64));
        if let Some(pid) = project_id {
            query = query.bind(("project_id", pid.to_string()));
        }
        if let Some(gid) = group_id {
            query = query.bind(("group_id", gid.to_string()));
        }

        let records: Vec<EpisodeRecord> = query.await?.take(0).unwrap_or_default();
        let episodes = records.into_iter().map(|r| r.into_episode()).collect();
        Ok(episodes)
    }

    /// Search episodes by content using BM25 full-text search.
    /// Falls back to CONTAINS-based search when BM25 is unavailable.
    pub async fn search_episodes(
        &self,
        query: &str,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Episode>> {
        // Try BM25 first, fall back to CONTAINS
        let result = self.search_episodes_bm25(query, project_id, limit).await;
        match result {
            Ok(episodes) => Ok(episodes),
            Err(e) => {
                tracing::warn!(error = %e, "BM25 FTS unavailable for episodes, falling back to CONTAINS search");
                self.search_episodes_fallback(query, project_id, limit)
                    .await
            }
        }
    }

    async fn search_episodes_bm25(
        &self,
        query: &str,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Episode>> {
        let sql = if project_id.is_some() {
            "SELECT *, search::score() AS _score FROM episode \
             WHERE content @@ $query AND project_id = $pid \
             ORDER BY _score DESC LIMIT $limit"
        } else {
            "SELECT *, search::score() AS _score FROM episode \
             WHERE content @@ $query \
             ORDER BY _score DESC LIMIT $limit"
        };

        let mut q = self
            .db
            .query(sql)
            .bind(("query", query.to_string()))
            .bind(("limit", limit as i64));
        if let Some(pid) = project_id {
            q = q.bind(("pid", pid.to_string()));
        }

        // BM25 queries add _score — we can't use the typed EpisodeRecord which doesn't
        // have that field, so fall back to serde_json::Value and parse manually.
        // On kv-mem (tests) this returns empty (BM25 not supported), triggering fallback.
        let rows: Vec<serde_json::Value> = q.await?.take(0).unwrap_or_default();
        let episodes: Vec<Episode> = rows
            .iter()
            .filter_map(|row| {
                // Remove _score field before parsing so EpisodeRecord doesn't choke on it
                parse_episode_row(row).ok()
            })
            .collect();
        Ok(episodes)
    }

    async fn search_episodes_fallback(
        &self,
        query: &str,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Episode>> {
        let sql = if project_id.is_some() {
            "SELECT * FROM episode \
             WHERE string::lowercase(content) CONTAINS string::lowercase($query) \
               AND project_id = $pid \
             ORDER BY reference_time DESC LIMIT $limit"
        } else {
            "SELECT * FROM episode \
             WHERE string::lowercase(content) CONTAINS string::lowercase($query) \
             ORDER BY reference_time DESC LIMIT $limit"
        };

        let mut q = self
            .db
            .query(sql)
            .bind(("query", query.to_string()))
            .bind(("limit", limit as i64));
        if let Some(pid) = project_id {
            q = q.bind(("pid", pid.to_string()));
        }

        let records: Vec<EpisodeRecord> = q.await?.take(0).unwrap_or_default();
        let episodes = records.into_iter().map(|r| r.into_episode()).collect();
        Ok(episodes)
    }

    pub async fn invalidate_note_at(&self, id: &str, at: DateTime<Utc>) -> Result<()> {
        // Strip the "note:" prefix if present, then build a typed RecordId
        let bare_id = id.strip_prefix("note:").unwrap_or(id);
        let rid = RecordId::new("note", bare_id);

        let sql = "UPDATE $rid SET invalid_at = $at, status = 'obsolete'";

        self.db
            .query(sql)
            .bind(("rid", rid))
            .bind(("at", at))
            .await?;
        Ok(())
    }

    pub async fn search_notes_at_time(
        &self,
        query: &str,
        at: DateTime<Utc>,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Note>> {
        // Try BM25 first.  If BM25 succeeds with results, return them.
        // If BM25 returns empty (possible when the kv-mem engine lacks FTS index)
        // or returns an error, fall back to keyword CONTAINS search.
        match self
            .search_notes_at_time_bm25(query, at, project_id, limit)
            .await
        {
            Ok(notes) if !notes.is_empty() => Ok(notes),
            Ok(_) => {
                tracing::debug!("BM25 returned no results, falling back to CONTAINS search");
                self.search_notes_at_time_fallback(query, at, project_id, limit)
                    .await
            }
            Err(e) => {
                tracing::warn!(error = %e, "BM25 FTS unavailable, falling back to CONTAINS search");
                self.search_notes_at_time_fallback(query, at, project_id, limit)
                    .await
            }
        }
    }

    async fn search_notes_at_time_bm25(
        &self,
        query: &str,
        at: DateTime<Utc>,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Note>> {
        let sql = if project_id.is_some() {
            "SELECT meta::id(id) AS uid, project_id, note_type, status, \
             importance, content, tags, scope_type, scope_path, \
             staleness_score, energy, created_at, created_by, \
             confirmed_at, confirmed_by, last_activated, \
             changes_json, assertion_rule_json, assertion_result_json, \
             search::score() AS _score \
             FROM note \
             WHERE content @@ $query \
               AND (valid_at IS NONE OR valid_at <= $at) \
               AND (invalid_at IS NONE OR invalid_at > $at) \
               AND project_id = $project_id \
             ORDER BY _score DESC LIMIT $limit"
        } else {
            "SELECT meta::id(id) AS uid, project_id, note_type, status, \
             importance, content, tags, scope_type, scope_path, \
             staleness_score, energy, created_at, created_by, \
             confirmed_at, confirmed_by, last_activated, \
             changes_json, assertion_rule_json, assertion_result_json, \
             search::score() AS _score \
             FROM note \
             WHERE content @@ $query \
               AND (valid_at IS NONE OR valid_at <= $at) \
               AND (invalid_at IS NONE OR invalid_at > $at) \
             ORDER BY _score DESC LIMIT $limit"
        };

        let mut q = self
            .db
            .query(sql)
            .bind(("query", query.to_string()))
            .bind(("at", at))
            .bind(("limit", limit as i64));
        if let Some(pid) = project_id {
            q = q.bind(("project_id", pid.to_string()));
        }

        let rows: Vec<serde_json::Value> = q.await?.take(0).unwrap_or_default();
        let notes: Vec<Note> = rows
            .iter()
            .filter_map(|row| json_value_to_note(row).ok())
            .collect();
        Ok(notes)
    }

    async fn search_notes_at_time_fallback(
        &self,
        query: &str,
        at: DateTime<Utc>,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Note>> {
        // Use SELECT * so the result can be deserialized into the typed NoteRecord struct.
        // The temporal filter checks: note must not have been invalidated before `at`.
        let sql = if project_id.is_some() {
            "SELECT * FROM note \
             WHERE string::lowercase(content) CONTAINS string::lowercase($query) \
               AND (invalid_at IS NONE OR invalid_at > $at) \
               AND project_id = $project_id \
             LIMIT $limit"
        } else {
            "SELECT * FROM note \
             WHERE string::lowercase(content) CONTAINS string::lowercase($query) \
               AND (invalid_at IS NONE OR invalid_at > $at) \
             LIMIT $limit"
        };

        let mut q = self
            .db
            .query(sql)
            .bind(("query", query.to_string()))
            .bind(("at", at))
            .bind(("limit", limit as i64));
        if let Some(pid) = project_id {
            q = q.bind(("project_id", pid.to_string()));
        }

        let records: Vec<crate::note::NoteRecord> = q.await?.take(0).unwrap_or_default();
        let notes: Vec<Note> = records
            .into_iter()
            .filter_map(|r| r.into_note().ok())
            .collect();
        Ok(notes)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;
    use chrono::Duration;
    use cortex_core::notes::{NoteStatus, NoteType};

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store.ensure_note_schema_extensions().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_add_and_get_episode() {
        let store = setup().await;

        let req = CreateEpisodeRequest {
            name: "Test conversation".to_string(),
            content: "We discussed the new authentication system".to_string(),
            source: EpisodeSource::Conversation,
            reference_time: None,
            project_id: None,
            group_id: None,
        };

        let episode = store.add_episode(req).await.unwrap();

        // Verify returned episode has correct fields
        assert!(!episode.id.is_empty(), "episode id should be non-empty");
        assert!(
            episode.id.starts_with("episode:"),
            "episode id should start with 'episode:'"
        );
        assert_eq!(episode.name, "Test conversation");
        assert_eq!(
            episode.content,
            "We discussed the new authentication system"
        );
        assert_eq!(episode.source, EpisodeSource::Conversation);
        assert!(episode.project_id.is_none());

        // Verify it appears in get_episodes
        let episodes = store.get_episodes(None, None, 10).await.unwrap();
        assert_eq!(episodes.len(), 1);
        assert_eq!(episodes[0].name, "Test conversation");
        assert_eq!(
            episodes[0].content,
            "We discussed the new authentication system"
        );
    }

    #[tokio::test]
    async fn test_get_episodes_project_filter() {
        let store = setup().await;

        // Episode with project_id = "proj-A"
        let req_a = CreateEpisodeRequest {
            name: "Project A episode".to_string(),
            content: "This belongs to project A".to_string(),
            source: EpisodeSource::Code,
            reference_time: None,
            project_id: Some("proj-A".to_string()),
            group_id: None,
        };
        store.add_episode(req_a).await.unwrap();

        // Episode without project_id
        let req_global = CreateEpisodeRequest {
            name: "Global episode".to_string(),
            content: "This is a global episode".to_string(),
            source: EpisodeSource::Event,
            reference_time: None,
            project_id: None,
            group_id: None,
        };
        store.add_episode(req_global).await.unwrap();

        // Filter by project-A — should return only 1 result
        let results = store.get_episodes(Some("proj-A"), None, 10).await.unwrap();
        assert_eq!(results.len(), 1, "expected exactly 1 episode for proj-A");
        assert_eq!(results[0].project_id, Some("proj-A".to_string()));
        assert_eq!(results[0].name, "Project A episode");
    }

    #[tokio::test]
    async fn test_invalidate_note_at() {
        let store = setup().await;

        // Create a note using the same pattern as note.rs tests
        let note = cortex_core::notes::Note::new(
            Some(uuid::Uuid::new_v4()),
            NoteType::Guideline,
            "This note will be invalidated".to_string(),
            "test-agent".to_string(),
        );
        store.create_note(&note).await.unwrap();

        // Verify the note is active before invalidation
        let before = store.get_note(note.id).await.unwrap().unwrap();
        assert_eq!(before.status, NoteStatus::Active);

        // Invalidate the note now
        let bare_id = note.id.to_string();
        store
            .invalidate_note_at(&bare_id, Utc::now())
            .await
            .unwrap();

        // Retrieve the note again and verify it was marked obsolete
        let after = store.get_note(note.id).await.unwrap().unwrap();
        assert_eq!(
            after.status,
            NoteStatus::Obsolete,
            "status should be 'obsolete' after invalidation"
        );
    }

    #[tokio::test]
    async fn test_search_notes_at_time() {
        let store = setup().await;

        let keyword = "authentication";

        // Create 2 notes with the same keyword in content
        let note1 = cortex_core::notes::Note::new(
            None,
            NoteType::Pattern,
            "Use JWT for authentication tokens".to_string(),
            "agent".to_string(),
        );
        let note2 = cortex_core::notes::Note::new(
            None,
            NoteType::Guideline,
            "authentication middleware must validate expiry".to_string(),
            "agent".to_string(),
        );
        store.create_note(&note1).await.unwrap();
        store.create_note(&note2).await.unwrap();

        // Invalidate note2 one hour ago (it was valid 2 hours ago, but not now)
        let one_hour_ago = Utc::now() - Duration::hours(1);
        store
            .invalidate_note_at(&note2.id.to_string(), one_hour_ago)
            .await
            .unwrap();

        // Query 2 hours ago — note2 was still valid then (invalidated 1 hour ago)
        let two_hours_ago = Utc::now() - Duration::hours(2);
        let historic_results = store
            .search_notes_at_time(keyword, two_hours_ago, None, 10)
            .await
            .unwrap();
        let historic_ids: Vec<uuid::Uuid> = historic_results.iter().map(|n| n.id).collect();
        assert!(
            historic_ids.contains(&note1.id),
            "note1 should appear in historic query (2 hours ago)"
        );
        assert!(
            historic_ids.contains(&note2.id),
            "note2 should appear in historic query (2 hours ago), before it was invalidated"
        );

        // Query now — note2 was invalidated 1 hour ago, so it should not appear
        let now_results = store
            .search_notes_at_time(keyword, Utc::now(), None, 10)
            .await
            .unwrap();
        let now_ids: Vec<uuid::Uuid> = now_results.iter().map(|n| n.id).collect();
        assert!(
            now_ids.contains(&note1.id),
            "note1 should still appear in current query"
        );
        assert!(
            !now_ids.contains(&note2.id),
            "note2 should NOT appear in current query (was invalidated 1 hour ago)"
        );
    }
}

fn parse_episode_row(row: &serde_json::Value) -> Result<Episode> {
    let id = row["id"].as_str().unwrap_or("").to_string();
    let name = row["name"].as_str().unwrap_or("").to_string();
    let content = row["content"].as_str().unwrap_or("").to_string();
    let source_str = row["source"].as_str().unwrap_or("event");
    let source = source_str
        .parse::<EpisodeSource>()
        .unwrap_or(EpisodeSource::Event);

    let reference_time = row["reference_time"]
        .as_str()
        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        .unwrap_or_else(Utc::now);
    let ingested_at = row["ingested_at"]
        .as_str()
        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        .unwrap_or_else(Utc::now);

    let project_id = row["project_id"].as_str().map(|s| s.to_string());
    let group_id = row["group_id"].as_str().map(|s| s.to_string());

    Ok(Episode {
        id,
        name,
        content,
        source,
        reference_time,
        ingested_at,
        project_id,
        group_id,
    })
}
