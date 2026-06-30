//! Queue-based episode ingest (NATS).
//!
//! Lets external producers stream documents into episodic memory without HTTP —
//! e.g. the conflux Outlook→Gold pipeline bridges each email onto
//! [`EPISODE_INGEST_SUBJECT`]. Each message is a JSON
//! [`CreateEpisodeRequest`](cortex_core::episode::CreateEpisodeRequest), so the
//! producer supplies `allowed_principals` (ADR-220) — the mailbox owner for
//! personal mail — and the same ACL trimming applies as on the HTTP path.

use std::sync::Arc;

use cortex_core::episode::CreateEpisodeRequest;
use futures::StreamExt;

use crate::indentiagraph::GraphStore;

/// NATS subject external producers publish episode-ingest messages to.
pub const EPISODE_INGEST_SUBJECT: &str = "cortex.ingest.episode";

/// Spawn a background task that subscribes to [`EPISODE_INGEST_SUBJECT`] and
/// ingests each JSON `CreateEpisodeRequest` into episodic memory.
///
/// Best-effort: a bad payload or a store error is logged and the message is
/// dropped — the listener never crashes the server. Visibility of each ingested
/// episode is governed by its `allowed_principals` (ADR-220).
pub fn spawn_episode_ingest(client: async_nats::Client, store: Arc<dyn GraphStore>) {
    tokio::spawn(async move {
        let mut sub = match client.subscribe(EPISODE_INGEST_SUBJECT).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, subject = EPISODE_INGEST_SUBJECT, "episode queue-ingest subscribe failed");
                return;
            }
        };
        tracing::info!(subject = EPISODE_INGEST_SUBJECT, "episode queue-ingest listening");

        while let Some(msg) = sub.next().await {
            match serde_json::from_slice::<CreateEpisodeRequest>(&msg.payload) {
                Ok(req) => {
                    let label = req.name.clone();
                    match store.add_episode(req).await {
                        Ok(ep) => tracing::debug!(episode = %ep.id, name = %label, "queue-ingested episode"),
                        Err(e) => tracing::warn!(error = %e, name = %label, "episode queue-ingest store failed"),
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, bytes = msg.payload.len(), "episode queue-ingest: undecodable payload");
                }
            }
        }
        tracing::warn!(subject = EPISODE_INGEST_SUBJECT, "episode queue-ingest subscription ended");
    });
}
