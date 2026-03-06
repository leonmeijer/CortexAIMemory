# CortexAIMemory: IndentiaGraph → IndentiaGraph Migration Plan

## Context

CortexAIMemory currently depends on IndentiaGraph (via `neo4rs`) as its graph database backend. The project has already been restructured into a Cargo workspace with clean abstractions:

- **`cortex-core`** — Pure domain types (no I/O)
- **`cortex-graph`** — `GraphStore` trait (368+ methods) + `MockGraphStore` + embeddings
- **Root crate** — IndentiaGraphClient implementation + API + orchestration

The `GraphStore` trait provides a clean abstraction boundary. We need to create a new `IndentiaGraphStore` implementation backed by SurrealDB (IndentiaGraph's storage engine) that replaces `IndentiaGraphClient` — and add comprehensive tests to verify correctness.

**Why**: IndentiaGraph (SurrealDB v3) offers native Rust integration, embedded mode for zero-dependency development, RDF/SPARQL capabilities, built-in full-text search, vector search, and Raft clustering — eliminating the need for a separate IndentiaGraph instance.

---

## Architecture Decisions

### 1. Data Model: SurrealDB Tables (not RDF triples)
Each node type maps to a SurrealDB **table** (e.g., `project`, `file`, `function`). Relationships use SurrealDB's native **graph edges** (`RELATE`). This is simpler and more performant than encoding everything as RDF triples, and maps 1:1 to the existing IndentiaGraph model.

### 2. Query Language: SurrealQL
All queries use native SurrealQL. No SPARQL needed — the graph model is domain-specific, not RDF. SurrealQL supports graph traversal (`->`, `<-`), record links, and `RELATE` for edges natively.

### 3. Connection Model: Direct `surrealdb` crate (Docker from start)
Use the `surrealdb` crate directly with Docker SurrealDB from the beginning:
- **Docker SurrealDB** — for development AND tests (real DB from day one)
- **Embedded `SurrealKV`** — optional for offline/local use
- **Remote `ws://`** — for production (connect to IndentiaGraph cluster)

Add SurrealDB to `docker-compose.yml` in Phase 1 alongside IndentiaGraph and Meilisearch.

### 4. Vector Search: SurrealDB native `MTREE` index
SurrealDB v3 supports `DEFINE INDEX ... MTREE DIMENSION 768 DIST COSINE` for vector similarity search, replacing IndentiaGraph's HNSW indexes.

### 5. Batch Operations: SurrealQL transactions
Replace IndentiaGraph `UNWIND` with SurrealDB `BEGIN TRANSACTION ... COMMIT` and parameterized bulk inserts.

---

## New Crate: `crates/cortex-indentiagraph/`

```
crates/cortex-indentiagraph/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Exports + IndentiaGraphStore struct
│   ├── client.rs           # Connection setup, schema init, helpers
│   ├── schema.rs           # DEFINE TABLE/FIELD/INDEX statements
│   ├── project.rs          # Project CRUD (8 methods)
│   ├── workspace.rs        # Workspace ops (10 methods)
│   ├── workspace_milestone.rs  # WS milestone ops (15 methods)
│   ├── resource.rs         # Resource ops (8 methods)
│   ├── component.rs        # Component ops (9 methods)
│   ├── file.rs             # File CRUD + batch (10 methods)
│   ├── symbol.rs           # Function/Struct/Trait/Enum/Impl/Import (26 methods)
│   ├── code.rs             # Code exploration (48 methods)
│   ├── plan.rs             # Plan ops (30 methods)
│   ├── decision.rs         # Decision ops (14 methods)
│   ├── constraint.rs       # Constraint ops (5 methods)
│   ├── commit.rs           # Commit ops (7 methods)
│   ├── release.rs          # Release ops (6 methods)
│   ├── milestone.rs        # Milestone ops (12 methods)
│   ├── chat.rs             # Chat session ops (7 methods)
│   ├── note.rs             # Knowledge notes (34 methods)
│   ├── skill.rs            # Neural skills (14 methods)
│   ├── analytics.rs        # Graph analytics (15 methods)
│   ├── fabric.rs           # Knowledge fabric (16 methods)
│   ├── feature_graph.rs    # Feature graphs (8 methods)
│   ├── topology.rs         # Topology rules (5 methods)
│   ├── user.rs             # Authentication (11 methods)
│   ├── util.rs             # Utility + context cards (6 methods)
│   └── impl_graph_store.rs # GraphStore trait impl (delegates to modules)
```

---

## SurrealDB Schema Design

### Tables (one per node type)
```surql
-- Core entities
DEFINE TABLE project SCHEMAFULL;
DEFINE FIELD id ON project TYPE string;      -- UUID as string
DEFINE FIELD name ON project TYPE string;
DEFINE FIELD slug ON project TYPE string;
DEFINE FIELD root_path ON project TYPE string;
DEFINE FIELD description ON project TYPE option<string>;
DEFINE FIELD created_at ON project TYPE datetime;
DEFINE FIELD last_synced ON project TYPE option<datetime>;
DEFINE FIELD analytics_computed_at ON project TYPE option<datetime>;
DEFINE FIELD last_co_change_computed_at ON project TYPE option<datetime>;
DEFINE INDEX idx_project_id ON project FIELDS id UNIQUE;
DEFINE INDEX idx_project_slug ON project FIELDS slug UNIQUE;

-- Files, functions, structs, etc. follow same pattern...

-- Edges via RELATE
DEFINE TABLE contains SCHEMAFULL;           -- project -> file, file -> symbol
DEFINE TABLE imports SCHEMAFULL;            -- file -> file
DEFINE TABLE calls SCHEMAFULL;              -- function -> function
DEFINE TABLE depends_on SCHEMAFULL;         -- task -> task
DEFINE TABLE has_plan SCHEMAFULL;           -- project -> plan
DEFINE TABLE has_task SCHEMAFULL;           -- plan -> task
DEFINE TABLE synapse SCHEMAFULL;            -- note <-> note (weighted)
DEFINE TABLE attached_to SCHEMAFULL;        -- note -> entity
-- ... (all 40+ relationship types)

-- Vector indexes
DEFINE INDEX idx_note_embedding ON note FIELDS embedding MTREE DIMENSION 768 DIST COSINE;
DEFINE INDEX idx_file_embedding ON file FIELDS embedding MTREE DIMENSION 768 DIST COSINE;
DEFINE INDEX idx_function_embedding ON function FIELDS embedding MTREE DIMENSION 768 DIST COSINE;
DEFINE INDEX idx_decision_embedding ON decision FIELDS embedding MTREE DIMENSION 768 DIST COSINE;
```

### Query Pattern Translation (Cypher → SurrealQL)

| IndentiaGraph (Cypher) | SurrealDB (SurrealQL) |
|---|---|
| `CREATE (n:Project {id: $id, ...})` | `CREATE project SET id = $id, ...` |
| `MATCH (n:Project {id: $id}) RETURN n` | `SELECT * FROM project WHERE id = $id` |
| `MERGE (n:File {path: $p}) SET n.x = $x` | `UPSERT file SET path = $p, x = $x WHERE path = $p` |
| `MATCH (a)-[:IMPORTS]->(b)` | `SELECT ->imports->file FROM file WHERE id = $id` |
| `MATCH (a)-[:CALLS*2..5]->(b)` | `SELECT ->calls->function->calls->function... FROM ...` (or recursive) |
| `UNWIND $items AS item MERGE ...` | `BEGIN TRANSACTION; FOR $item IN $items { UPSERT ... }; COMMIT;` |
| `MATCH (n) WHERE n.embedding IS NOT NULL ...` (vector) | `SELECT * FROM note WHERE embedding <|768,COSINE|> $vec LIMIT $k` |

---

## Implementation Phases

### Phase 1: Foundation (Docker + client + schema + project CRUD)
**Files**: `lib.rs`, `client.rs`, `schema.rs`, `project.rs`, `impl_graph_store.rs`, `Cargo.toml`, `docker-compose.yml`

1. Add SurrealDB to `docker-compose.yml` (port 8000, root/root, namespace: cortex, database: memory)
2. Create `crates/cortex-indentiagraph/Cargo.toml` with deps: `surrealdb` (kv-mem + protocol-ws), `cortex-core`, `cortex-graph`, `tokio`, `anyhow`, `async-trait`, `uuid`, `chrono`, `serde`, `serde_json`, `tracing`
3. Implement `IndentiaGraphStore` struct with `surrealdb::Surreal<Client>` connection
4. Add `new()` for remote (ws://localhost:8000), `new_embedded()` for SurrealKV
5. Implement `init_schema()` — all DEFINE TABLE/FIELD/INDEX statements
6. Implement Project CRUD (8 methods) — first complete vertical slice against Docker SurrealDB
7. Wire up `impl GraphStore for IndentiaGraphStore` (starting with project methods)
8. **Tests**: Full unit + integration tests running against Docker SurrealDB

### Phase 2: Core Graph Structure (files, symbols, relationships)
**Files**: `file.rs`, `symbol.rs`

1. File operations (10 methods): CRUD, batch upsert, project linking
2. Symbol operations (26 methods): functions, structs, traits, enums, impls, imports
3. Relationship creation: CONTAINS, IMPORTS, CALLS, EXTENDS, IMPLEMENTS
4. Batch operations: bulk upsert with SurrealQL transactions
5. **Tests**: File/symbol CRUD, batch operations, relationship traversal

### Phase 3: Planning & Task Management
**Files**: `plan.rs`, `decision.rs`, `constraint.rs`, `commit.rs`, `release.rs`, `milestone.rs`

1. Plan CRUD + task management (30 methods)
2. Decision CRUD + affects tracking (14 methods)
3. Constraints, commits, releases, milestones (30 methods combined)
4. Task dependencies, blockers, critical path
5. **Tests**: Plan lifecycle, task dependencies, critical path calculation

### Phase 4: Knowledge System (notes, skills, fabric)
**Files**: `note.rs`, `skill.rs`, `fabric.rs`

1. Knowledge notes (34 methods): CRUD, linking, propagation, synapses, staleness
2. Neural skills (14 methods): CRUD, members, activation, triggers
3. Knowledge fabric (16 methods): co-changed, density, risk, neural metrics
4. Vector search implementation using MTREE indexes
5. **Tests**: Note lifecycle, synapse propagation, vector search accuracy, skill activation

### Phase 5: Code Exploration & Analytics
**Files**: `code.rs`, `analytics.rs`, `feature_graph.rs`, `topology.rs`

1. Code exploration (48 methods): references, callers, impact analysis, architecture
2. Analytics (15 methods): communities, health, fingerprints (reuse petgraph engine)
3. Feature graphs (8 methods): CRUD, auto-build
4. Topology rules (5 methods): CRUD, violation checks
5. **Tests**: Call graph traversal, impact analysis, community detection, circular deps

### Phase 6: Workspace, Chat, Auth & Utilities
**Files**: `workspace.rs`, `workspace_milestone.rs`, `resource.rs`, `component.rs`, `chat.rs`, `user.rs`, `util.rs`

1. Workspace operations (10 methods) + milestones (15 methods)
2. Resources (8 methods) + components (9 methods) + topology
3. Chat sessions (7 methods) + event storage
4. Authentication (11 methods): users, tokens
5. Utility (6 methods): health check, context cards
6. **Tests**: Workspace topology, cross-project milestones, auth flows

### Phase 7: Integration & Switchover
**Files**: root `Cargo.toml`, `src/main.rs`, `src/bin/mcp_server.rs`, `src/lib.rs`, `docker-compose.yml`

1. Add `cortex-indentiagraph` to workspace and root deps
2. Add feature flag: `indentiagraph` (default) vs `indentiagraph` (legacy)
3. Update `Config` with SurrealDB connection settings
4. Update `main.rs` to construct `IndentiaGraphStore` based on config
5. Update `docker-compose.yml` to run SurrealDB instead of IndentiaGraph
6. Update `mcp_server.rs` binary
7. Full end-to-end integration test suite
8. Update CLAUDE.md documentation

---

## Testing Strategy (Comprehensive)

The user specifically requested **many unit and integration tests**. Here's the testing plan:

### Unit Tests (in `crates/cortex-indentiagraph/src/`)

Each module file gets `#[cfg(test)] mod tests { ... }` with:

**Schema Tests** (`schema.rs`):
- Schema initialization creates all tables
- Schema initialization creates all indexes
- Schema is idempotent (running twice doesn't error)
- All field types are correctly defined

**Per-Entity CRUD Tests** (every module):
- `test_create_and_get_{entity}` — round-trip create → get
- `test_create_duplicate_{entity}` — unique constraint enforcement
- `test_get_nonexistent_{entity}` — returns None
- `test_update_{entity}` — partial update preserves unchanged fields
- `test_delete_{entity}` — removes entity and cascading relationships
- `test_list_{entities}` — pagination, filtering, sorting
- `test_batch_upsert_{entities}` — bulk operations (10, 100, 1000 items)

**Relationship Tests**:
- `test_create_relationship` — RELATE creates edge
- `test_delete_relationship` — edge removal
- `test_traverse_single_hop` — A→B traversal
- `test_traverse_multi_hop` — A→B→C→D traversal
- `test_bidirectional_traversal` — both directions
- `test_relationship_properties` — edge properties (weight, confidence)
- `test_cascading_delete` — deleting node removes edges

**Vector Search Tests**:
- `test_vector_insert_and_search` — store + retrieve by similarity
- `test_vector_cosine_similarity_ordering` — closest vectors first
- `test_vector_search_with_filters` — combine vector + field filters
- `test_vector_search_empty_index` — no panic on empty
- `test_vector_dimension_mismatch` — error handling

**Query Pattern Tests**:
- `test_filter_by_status` — enum filtering
- `test_filter_by_priority_range` — min/max priority
- `test_filter_by_tags` — tag intersection
- `test_pagination` — limit + offset correctness
- `test_sort_by_created_at` — ordering

**Concurrency Tests**:
- `test_concurrent_writes` — parallel create doesn't corrupt
- `test_concurrent_read_write` — reads during writes
- `test_batch_under_load` — large batches don't timeout

**Estimated unit tests: ~300+** (per-entity × per-operation × edge cases)

### Integration Tests (in `tests/`)

**`tests/indentiagraph_integration.rs`** — Full GraphStore contract tests:
- Run against embedded SurrealDB (`Mem` or `SurrealKV`)
- Test every public GraphStore method end-to-end
- Verify behavior matches MockGraphStore

**`tests/indentiagraph_migration.rs`** — Migration correctness:
- Create data via MockGraphStore, replicate to IndentiaGraphStore, verify equality
- Test all 75+ node types round-trip correctly
- Test all 40+ relationship types

**`tests/indentiagraph_analytics.rs`** — Analytics pipeline:
- Import a known code graph, run analytics, verify PageRank/betweenness values
- Test community detection produces expected clusters
- Test structural fingerprints are deterministic

**`tests/indentiagraph_knowledge.rs`** — Knowledge system:
- Note creation, linking, propagation across workspace
- Synapse creation and spreading activation
- Staleness scoring over time
- Skill emergence from note clusters

**GraphStore Contract Test Suite** — generic tests that run against ANY GraphStore impl:

```rust
// tests/graph_store_contract.rs
// Generic test functions that accept &dyn GraphStore
// Run once with MockGraphStore, once with IndentiaGraphStore

async fn contract_project_crud(store: &dyn GraphStore) { ... }
async fn contract_file_batch_upsert(store: &dyn GraphStore) { ... }
async fn contract_task_dependencies(store: &dyn GraphStore) { ... }
async fn contract_note_propagation(store: &dyn GraphStore) { ... }
// ... 50+ contract tests
```

This **contract test pattern** is the most valuable: it guarantees that IndentiaGraphStore behaves identically to MockGraphStore (which is the reference spec).

**Estimated integration tests: ~200+**

### Total Expected Tests: **500+ new tests**

---

## Critical Files to Modify

| File | Change |
|------|--------|
| `/Cargo.toml` (root workspace) | Add `crates/cortex-indentiagraph` to members |
| `/Cargo.toml` (root package) | Add `cortex-indentiagraph` dep, feature flag |
| `/crates/cortex-indentiagraph/` | **NEW** — entire crate |
| `/src/main.rs` | Config-based backend selection |
| `/src/bin/mcp_server.rs` | Config-based backend selection |
| `/src/lib.rs` | Re-export indentiagraph module |
| `/src/indentiagraph/mod.rs` | Feature-gate behind `indentiagraph` feature |
| `/docker-compose.yml` | Add SurrealDB service |
| `/config.yaml.example` | Add SurrealDB config section |
| `/CLAUDE.md` | Update docs |

## Existing Code to Reuse

- **`cortex_graph::GraphStore`** (`crates/cortex-graph/src/traits.rs`) — trait to implement
- **`cortex_graph::MockGraphStore`** (`crates/cortex-graph/src/mock.rs`) — reference spec + contract test counterpart
- **`cortex_core::models::*`** — all domain types unchanged
- **`cortex_core::graph::*`** — analytics types (AnalysisProfile, TopologyRule, etc.)
- **`cortex_core::test_helpers`** — test factories (test_project, test_plan, etc.)
- **`src/graph/engine.rs`** — petgraph analytics pipeline (reuse as-is, not DB-specific)
- **`src/graph/algorithms.rs`** — PageRank, betweenness, Louvain (pure petgraph, not DB-specific)

---

---

## Phase 8: Claude-Mem Integration

### Context

**claude-mem** (`/Users/leon/git/claude-mem`) is a Claude Code plugin (TypeScript/Bun, v10.5.2) that intercepts lifecycle events via 6 hooks and persists session memory:

- **SessionStart** → inject project context from memory
- **UserPromptSubmit** → track prompts, initialize SDK agent
- **PostToolUse** → capture tool observations (decisions, discoveries, changes)
- **Stop** → generate session summary (request, investigated, learned, completed, next_steps)

Currently stores in **SQLite** (`~/.claude-mem/claude-mem.db`) + **Chroma** (vector search). The user wants **IndentiaGraphDB** as the storage backend instead.

### Data Model Mapping: claude-mem → CortexAIMemory Knowledge Graph

| claude-mem Entity | CortexAIMemory Target | Notes |
|---|---|---|
| `sdk_sessions` | `ChatSessionNode` | Already exists — map `content_session_id` → `cli_session_id` |
| `observations` | `Note` (type: `observation`) | Observations become knowledge notes attached to files/functions |
| `session_summaries` | `Note` (type: `context`) + `ChatSessionNode` fields | Summary stored as note, linked to session |
| `user_prompts` | `ChatEventRecord` (type: `user_message`) | Already supported in chat event storage |
| `pending_messages` | In-memory queue (no persistence needed) | Worker processes async, no DB change |

### Architecture: Full Rust Rewrite (hooks + worker)

Replace the **entire** Bun/TypeScript stack (hooks + worker) with Rust:

1. **`cortex-mem` binary** — Worker daemon (axum HTTP, port 37777)
2. **`cortex-mem-hook` binary** — Compiled Rust hook scripts (replaces Node.js .cjs files)
3. **Writes to IndentiaGraphDB** via `IndentiaGraphStore` (connects to Docker SurrealDB)
4. **Provides context injection** at SessionStart by querying the knowledge graph
5. **Generates summaries** via Claude Agent SDK (Rust nexus-claude, already a dep)

This replaces: Node.js hooks, Bun runtime, SQLite, Chroma, express server — with pure Rust binaries using the same graph DB as CortexAIMemory. Zero runtime dependencies (no Node.js, no Bun, no Python).

### New Crate: `crates/cortex-mem/`

```
crates/cortex-mem/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Exports
│   ├── server.rs           # HTTP API (axum, port 37777) — worker daemon
│   ├── context.rs          # Context injection (query graph → markdown)
│   ├── observation.rs      # Tool observation → Note conversion
│   ├── session.rs          # Session lifecycle management
│   ├── summary.rs          # Session summary generation (nexus-claude)
│   ├── config.rs           # Settings (compatible with ~/.claude-mem/settings.json)
│   ├── hooks/
│   │   ├── mod.rs          # Hook dispatch (read stdin, route to handler)
│   │   ├── session_start.rs   # SessionStart: start worker, inject context
│   │   ├── prompt_submit.rs   # UserPromptSubmit: init session state
│   │   ├── post_tool_use.rs   # PostToolUse: capture observation
│   │   └── stop.rs            # Stop: summarize + finalize
│   └── main.rs             # Worker daemon entry point
```

### New Binaries

```toml
[[bin]]
name = "cortex-mem"          # Worker daemon
path = "crates/cortex-mem/src/main.rs"

[[bin]]
name = "cortex-mem-hook"     # Hook binary (called by Claude Code)
path = "crates/cortex-mem/src/hooks/main.rs"
```

### Hook Registration

Generate `hooks.json` for Claude Code that invokes the Rust `cortex-mem-hook` binary:

```json
{
  "hooks": [
    {
      "event": "SessionStart",
      "command": "/path/to/cortex-mem-hook session-start",
      "matcher": "startup|clear|compact"
    },
    {
      "event": "UserPromptSubmit",
      "command": "/path/to/cortex-mem-hook prompt-submit"
    },
    {
      "event": "PostToolUse",
      "command": "/path/to/cortex-mem-hook post-tool-use",
      "matcher": "*"
    },
    {
      "event": "Stop",
      "command": "/path/to/cortex-mem-hook stop"
    }
  ]
}
```

The hook binary reads normalized JSON from stdin, dispatches to the appropriate handler, makes HTTP calls to the `cortex-mem` worker daemon, and returns hook output on stdout.

### API Compatibility (drop-in replacement for claude-mem worker)

| Endpoint | Method | Action |
|---|---|---|
| `POST /api/sessions/init` | Initialize session in graph |
| `POST /api/sessions/observations` | Create Note from observation |
| `POST /api/sessions/{id}/init` | Start SDK agent for session |
| `POST /api/sessions/{id}/complete` | Finalize session, trigger summary |
| `GET /api/context/inject?projects=...` | Query graph for context markdown |
| `GET /api/search?q=...` | Vector + text search via IndentiaGraph |
| `GET /api/timeline?date=...` | Chronological note listing |
| `POST /api/admin/shutdown` | Graceful shutdown |

### Implementation Steps

1. Create `crates/cortex-mem/` crate with axum HTTP server
2. Implement session lifecycle (init → observations → complete → summary)
3. Implement observation → Note conversion with auto-linking to files/functions
4. Implement context injection: query recent notes/observations for project, format as markdown
5. Implement vector search via IndentiaGraph MTREE indexes
6. Implement summary generation via nexus-claude SDK
7. Add startup script that launches `cortex-mem` as daemon (replacing Bun worker)
8. Update `plugin/hooks/hooks.json` to use `cortex-mem` instead of Bun worker

### Tests for cortex-mem

**Hook Binary Unit Tests** (`hooks/`):
- `test_stdin_parsing` — parse all Claude Code hook input formats
- `test_session_start_output` — correct `additionalContext` format
- `test_post_tool_use_extraction` — tool name, input, response parsed
- `test_stop_output` — summary generation triggered
- `test_privacy_tag_stripping` — `<private>content</private>` removed before storage
- `test_unknown_event_handling` — graceful no-op for unrecognized events

**Worker Unit Tests** (`server.rs`, `observation.rs`, `session.rs`):
- `test_observation_to_note_conversion` — all 6 observation types map correctly
- `test_context_injection_formatting` — markdown output matches expected format
- `test_session_lifecycle` — init → observe → complete → summary flow
- `test_content_deduplication` — SHA-256 hash prevents duplicate observations
- `test_session_auto_continue` — session resume after reconnect
- `test_config_loading` — settings.json compatibility with claude-mem format

**Integration Tests** (against Docker SurrealDB):
- `test_full_hook_flow` — simulate SessionStart → PostToolUse × N → Stop
- `test_context_retrieval_across_sessions` — observations from session 1 appear in session 2 context
- `test_cross_project_context` — workspace-level note propagation via graph
- `test_search_observations` — vector + text search returns relevant results
- `test_concurrent_sessions` — multiple sessions writing simultaneously
- `test_hook_binary_e2e` — spawn `cortex-mem-hook` process, pipe stdin, verify stdout
- `test_worker_daemon_lifecycle` — start, serve requests, graceful shutdown

**Estimated tests: ~70+**

---

## Verification Plan

1. **`docker compose up -d surrealdb`** — SurrealDB running on port 8000
2. **`cargo test -p cortex-indentiagraph`** — all unit + integration tests pass against Docker SurrealDB
3. **`cargo test --test indentiagraph_integration`** — full GraphStore integration tests pass
4. **`cargo test --test indentiagraph_contract`** — contract tests pass on both Mock + IndentiaGraph
5. **`cargo test -p cortex-mem`** — cortex-mem hook + worker tests pass
6. **`cargo clippy -p cortex-indentiagraph -p cortex-mem`** — no warnings
7. **`cargo build --release`** — full project compiles with indentiagraph feature
8. **Manual**: Run `cortex serve`, create a project, sync files, verify code search works
9. **Manual**: Run MCP server (`cortex-mcp`), test via Claude Code that all mega-tools work
10. **Manual**: Start `cortex-mem` daemon, register hooks, open Claude Code session → verify observations stored
11. **Manual**: Open second Claude Code session → verify context injection includes previous observations
12. **Compare**: Run same operations on IndentiaGraph and IndentiaGraph, verify identical results

### Total New Tests: **570+** (300 unit + 200 integration + 70 cortex-mem)
