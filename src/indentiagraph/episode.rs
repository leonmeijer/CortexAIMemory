//! Episode storage for IndentiaGraph backend (Graphiti-inspired episodic memory)

use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::models::{CreateEpisodeRequest, EpisodeNode, EpisodeSource};
use crate::indentiagraph::client::IndentiaGraphClient;

impl IndentiaGraphClient {
    /// Ingest a new episode into episodic memory.
    pub async fn add_episode(&self, req: CreateEpisodeRequest) -> Result<EpisodeNode> {
        let id = Uuid::new_v4();
        let reference_time = req.reference_time.unwrap_or_else(Utc::now);
        let ingested_at = Utc::now();

        // Build query with optional project_id and group_id params
        let project_id_str = req.project_id.map(|p| p.to_string());
        let group_id_str = req.group_id.clone();

        let q = neo4rs::query(
            r#"
            CREATE (e:Episode {
                id: $id,
                name: $name,
                content: $content,
                source: $source,
                reference_time: $reference_time,
                ingested_at: $ingested_at,
                project_id: $project_id,
                group_id: $group_id
            })
            RETURN e
            "#,
        )
        .param("id", id.to_string())
        .param("name", req.name.clone())
        .param("content", req.content.clone())
        .param("source", req.source.to_string())
        .param("reference_time", reference_time.to_rfc3339())
        .param("ingested_at", ingested_at.to_rfc3339())
        .param(
            "project_id",
            project_id_str
                .clone()
                .map(neo4rs::BoltType::from)
                .unwrap_or(neo4rs::BoltType::Null(neo4rs::BoltNull)),
        )
        .param(
            "group_id",
            group_id_str
                .clone()
                .map(neo4rs::BoltType::from)
                .unwrap_or(neo4rs::BoltType::Null(neo4rs::BoltNull)),
        );

        let _result = self.graph.execute(q).await?;

        // If project_id is provided, link the episode to the project
        if let Some(pid) = req.project_id {
            let link_q = neo4rs::query(
                r#"
                MATCH (e:Episode {id: $episode_id})
                MATCH (p:Project {id: $project_id})
                MERGE (p)-[:HAS_EPISODE]->(e)
                "#,
            )
            .param("episode_id", id.to_string())
            .param("project_id", pid.to_string());

            // Best-effort — project may not exist in graph yet
            let _ = self.graph.run(link_q).await;
        }

        Ok(EpisodeNode {
            id,
            name: req.name,
            content: req.content,
            source: req.source,
            reference_time,
            ingested_at,
            project_id: req.project_id,
            group_id: req.group_id,
        })
    }

    /// Get recent episodes, optionally filtered by project or group.
    pub async fn get_episodes(
        &self,
        project_id: Option<Uuid>,
        group_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EpisodeNode>> {
        let (cypher, q) = if let Some(pid) = project_id {
            let q = neo4rs::query(
                "MATCH (e:Episode {project_id: $project_id}) RETURN e ORDER BY e.reference_time DESC LIMIT $limit",
            )
            .param("project_id", pid.to_string())
            .param("limit", limit as i64);
            ("project", q)
        } else if let Some(gid) = group_id {
            let q = neo4rs::query(
                "MATCH (e:Episode {group_id: $group_id}) RETURN e ORDER BY e.reference_time DESC LIMIT $limit",
            )
            .param("group_id", gid.to_string())
            .param("limit", limit as i64);
            ("group", q)
        } else {
            let q = neo4rs::query(
                "MATCH (e:Episode) RETURN e ORDER BY e.reference_time DESC LIMIT $limit",
            )
            .param("limit", limit as i64);
            ("all", q)
        };

        let _ = cypher; // suppress unused warning
        let mut result = self.graph.execute(q).await?;
        let mut episodes = Vec::new();
        while let Some(row) = result.next().await? {
            if let Ok(node) = row.get::<neo4rs::Node>("e") {
                if let Ok(ep) = parse_episode_node(&node) {
                    episodes.push(ep);
                }
            }
        }
        Ok(episodes)
    }

    /// Search episodes by content using CONTAINS (IndentiaGraph has no native BM25).
    pub async fn search_episodes(
        &self,
        query: &str,
        project_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<EpisodeNode>> {
        let q = if let Some(pid) = project_id {
            neo4rs::query(
                "MATCH (e:Episode) \
                 WHERE toLower(e.content) CONTAINS toLower($query) \
                   AND e.project_id = $project_id \
                 RETURN e ORDER BY e.reference_time DESC LIMIT $limit",
            )
            .param("query", query.to_string())
            .param("project_id", pid.to_string())
            .param("limit", limit as i64)
        } else {
            neo4rs::query(
                "MATCH (e:Episode) \
                 WHERE toLower(e.content) CONTAINS toLower($query) \
                 RETURN e ORDER BY e.reference_time DESC LIMIT $limit",
            )
            .param("query", query.to_string())
            .param("limit", limit as i64)
        };

        let mut result = self.graph.execute(q).await?;
        let mut episodes = Vec::new();
        while let Some(row) = result.next().await? {
            if let Ok(node) = row.get::<neo4rs::Node>("e") {
                if let Ok(ep) = parse_episode_node(&node) {
                    episodes.push(ep);
                }
            }
        }
        Ok(episodes)
    }
}

fn parse_episode_node(node: &neo4rs::Node) -> Result<EpisodeNode> {
    let id_str: String = node.get("id").unwrap_or_default();
    let id = id_str.parse::<Uuid>().unwrap_or_else(|_| Uuid::new_v4());
    let name: String = node.get("name").unwrap_or_default();
    let content: String = node.get("content").unwrap_or_default();
    let source_str: String = node.get("source").unwrap_or_else(|_| "event".to_string());
    let source = source_str
        .parse::<EpisodeSource>()
        .unwrap_or(EpisodeSource::Event);

    let reference_time_str: String = node.get("reference_time").unwrap_or_default();
    let reference_time = reference_time_str
        .parse::<DateTime<Utc>>()
        .unwrap_or_else(|_| Utc::now());
    let ingested_at_str: String = node.get("ingested_at").unwrap_or_default();
    let ingested_at = ingested_at_str
        .parse::<DateTime<Utc>>()
        .unwrap_or_else(|_| Utc::now());

    let project_id = node.get::<String>("project_id").ok().and_then(|s| {
        if s.is_empty() {
            None
        } else {
            s.parse::<Uuid>().ok()
        }
    });
    let group_id: Option<String> =
        node.get("group_id")
            .ok()
            .and_then(|s: String| if s.is_empty() { None } else { Some(s) });

    Ok(EpisodeNode {
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
