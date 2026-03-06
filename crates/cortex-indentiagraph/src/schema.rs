//! SurrealDB schema definitions.
//!
//! Defines all tables, fields, indexes, and edge tables for the CortexAIMemory graph.
//! Schema initialization is idempotent — safe to run multiple times.

use crate::client::IndentiaGraphStore;
use anyhow::{Context, Result};

impl IndentiaGraphStore {
    /// Initialize the full schema — tables, fields, indexes, edge tables.
    ///
    /// This is idempotent: running it multiple times has no effect.
    pub async fn init_schema(&self) -> Result<()> {
        self.init_node_tables().await?;
        self.init_edge_tables().await?;
        self.init_vector_indexes().await?;
        self.init_fts_indexes().await?;
        self.init_episode_schema().await?;
        Ok(())
    }

    /// Define all node tables with fields and indexes.
    async fn init_node_tables(&self) -> Result<()> {
        self.db
            .query(
                r#"
-- ================================================================
-- Project
-- ================================================================
DEFINE TABLE IF NOT EXISTS `project` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `project` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `project` TYPE string;
DEFINE FIELD IF NOT EXISTS slug ON `project` TYPE string;
DEFINE FIELD IF NOT EXISTS root_path ON `project` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `project` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `project` TYPE string;
DEFINE FIELD IF NOT EXISTS last_synced ON `project` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS analytics_computed_at ON `project` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS last_co_change_computed_at ON `project` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_project_id ON `project` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_project_slug ON `project` FIELDS slug UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_project_name ON `project` FIELDS name;

-- ================================================================
-- Workspace
-- ================================================================
DEFINE TABLE IF NOT EXISTS `workspace` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `workspace` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `workspace` TYPE string;
DEFINE FIELD IF NOT EXISTS slug ON `workspace` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `workspace` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `workspace` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `workspace` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS metadata ON `workspace` TYPE option<object> FLEXIBLE;
DEFINE INDEX IF NOT EXISTS idx_workspace_id ON `workspace` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_workspace_slug ON `workspace` FIELDS slug UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_workspace_name ON `workspace` FIELDS name;

-- ================================================================
-- File
-- ================================================================
DEFINE TABLE IF NOT EXISTS `file` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS path ON `file` TYPE string;
DEFINE FIELD IF NOT EXISTS language ON `file` TYPE string;
DEFINE FIELD IF NOT EXISTS hash ON `file` TYPE string;
DEFINE FIELD IF NOT EXISTS last_parsed ON `file` TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON `file` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS embedding ON `file` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS embedding_model ON `file` TYPE option<string>;
-- Analytics computed properties
DEFINE FIELD IF NOT EXISTS pagerank ON `file` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS betweenness ON `file` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS clustering_coeff ON `file` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS community_id ON `file` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS community_label ON `file` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS structural_dna ON `file` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS wl_hash ON `file` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS fingerprint ON `file` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS in_degree ON `file` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS out_degree ON `file` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS knowledge_density ON `file` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS risk_score ON `file` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS risk_level ON `file` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS risk_factors ON `file` TYPE option<object> FLEXIBLE;
DEFINE FIELD IF NOT EXISTS churn_score ON `file` TYPE option<float>;
-- Fabric analytics (multi-layer graph)
DEFINE FIELD IF NOT EXISTS fabric_pagerank ON `file` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS fabric_betweenness ON `file` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS fabric_community_id ON `file` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS fabric_community_label ON `file` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS fabric_clustering_coefficient ON `file` TYPE option<float>;
DEFINE INDEX IF NOT EXISTS idx_file_path ON `file` FIELDS path UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_file_project ON `file` FIELDS project_id;
DEFINE INDEX IF NOT EXISTS idx_file_language ON `file` FIELDS language;
DEFINE INDEX IF NOT EXISTS idx_file_wl_hash ON `file` FIELDS wl_hash;

-- ================================================================
-- Function
-- ================================================================
DEFINE TABLE IF NOT EXISTS `function` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `function` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `function` TYPE string;
DEFINE FIELD IF NOT EXISTS visibility ON `function` TYPE string;
DEFINE FIELD IF NOT EXISTS is_async ON `function` TYPE bool;
DEFINE FIELD IF NOT EXISTS is_unsafe ON `function` TYPE option<bool>;
DEFINE FIELD IF NOT EXISTS generics ON `function` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS parameters ON `function` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS return_type ON `function` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS file_path ON `function` TYPE string;
DEFINE FIELD IF NOT EXISTS line_start ON `function` TYPE int;
DEFINE FIELD IF NOT EXISTS line_end ON `function` TYPE int;
DEFINE FIELD IF NOT EXISTS body_hash ON `function` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS complexity ON `function` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS docstring ON `function` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS signature ON `function` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS embedding ON `function` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS embedding_model ON `function` TYPE option<string>;
-- Analytics
DEFINE FIELD IF NOT EXISTS pagerank ON `function` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS betweenness ON `function` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS community_id ON `function` TYPE option<int>;
DEFINE INDEX IF NOT EXISTS idx_function_id ON `function` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_function_name ON `function` FIELDS name;
DEFINE INDEX IF NOT EXISTS idx_function_file ON `function` FIELDS file_path;
DEFINE INDEX IF NOT EXISTS idx_function_name_file ON `function` FIELDS name, file_path;

-- ================================================================
-- Struct
-- ================================================================
DEFINE TABLE IF NOT EXISTS `struct` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `struct` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `struct` TYPE string;
DEFINE FIELD IF NOT EXISTS visibility ON `struct` TYPE string;
DEFINE FIELD IF NOT EXISTS generics ON `struct` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS file_path ON `struct` TYPE string;
DEFINE FIELD IF NOT EXISTS line_start ON `struct` TYPE int;
DEFINE FIELD IF NOT EXISTS line_end ON `struct` TYPE int;
DEFINE FIELD IF NOT EXISTS docstring ON `struct` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS parent_class ON `struct` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS interfaces ON `struct` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_struct_id ON `struct` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_struct_name ON `struct` FIELDS name;
DEFINE INDEX IF NOT EXISTS idx_struct_file ON `struct` FIELDS file_path;

-- ================================================================
-- Trait
-- ================================================================
DEFINE TABLE IF NOT EXISTS `trait` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `trait` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `trait` TYPE string;
DEFINE FIELD IF NOT EXISTS visibility ON `trait` TYPE string;
DEFINE FIELD IF NOT EXISTS generics ON `trait` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS file_path ON `trait` TYPE string;
DEFINE FIELD IF NOT EXISTS line_start ON `trait` TYPE int;
DEFINE FIELD IF NOT EXISTS line_end ON `trait` TYPE int;
DEFINE FIELD IF NOT EXISTS docstring ON `trait` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS is_external ON `trait` TYPE bool;
DEFINE FIELD IF NOT EXISTS source ON `trait` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_trait_id ON `trait` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_trait_name ON `trait` FIELDS name;

-- ================================================================
-- Enum
-- ================================================================
DEFINE TABLE IF NOT EXISTS `enum` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `enum` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `enum` TYPE string;
DEFINE FIELD IF NOT EXISTS visibility ON `enum` TYPE string;
DEFINE FIELD IF NOT EXISTS variants ON `enum` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS file_path ON `enum` TYPE string;
DEFINE FIELD IF NOT EXISTS line_start ON `enum` TYPE int;
DEFINE FIELD IF NOT EXISTS line_end ON `enum` TYPE int;
DEFINE FIELD IF NOT EXISTS docstring ON `enum` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_enum_id ON `enum` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_enum_name ON `enum` FIELDS name;

-- ================================================================
-- Impl
-- ================================================================
DEFINE TABLE IF NOT EXISTS `impl` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `impl` TYPE string;
DEFINE FIELD IF NOT EXISTS for_type ON `impl` TYPE string;
DEFINE FIELD IF NOT EXISTS trait_name ON `impl` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS generics ON `impl` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS where_clause ON `impl` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS file_path ON `impl` TYPE string;
DEFINE FIELD IF NOT EXISTS line_start ON `impl` TYPE int;
DEFINE FIELD IF NOT EXISTS line_end ON `impl` TYPE int;
DEFINE INDEX IF NOT EXISTS idx_impl_id ON `impl` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_impl_for_type ON `impl` FIELDS for_type;
DEFINE INDEX IF NOT EXISTS idx_impl_trait ON `impl` FIELDS trait_name;

-- ================================================================
-- Import
-- ================================================================
DEFINE TABLE IF NOT EXISTS `import` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `import` TYPE string;
DEFINE FIELD IF NOT EXISTS path ON `import` TYPE string;
DEFINE FIELD IF NOT EXISTS alias ON `import` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS items ON `import` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS file_path ON `import` TYPE string;
DEFINE FIELD IF NOT EXISTS line ON `import` TYPE int;
DEFINE INDEX IF NOT EXISTS idx_import_id ON `import` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_import_file ON `import` FIELDS file_path;

-- ================================================================
-- Plan
-- ================================================================
DEFINE TABLE IF NOT EXISTS `plan` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `plan` TYPE string;
DEFINE FIELD IF NOT EXISTS title ON `plan` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `plan` TYPE string;
DEFINE FIELD IF NOT EXISTS status ON `plan` TYPE string;
DEFINE FIELD IF NOT EXISTS priority ON `plan` TYPE int;
DEFINE FIELD IF NOT EXISTS created_by ON `plan` TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON `plan` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `plan` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS project_id ON `plan` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_plan_id ON `plan` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_plan_status ON `plan` FIELDS status;
DEFINE INDEX IF NOT EXISTS idx_plan_project ON `plan` FIELDS project_id;

-- ================================================================
-- Task
-- ================================================================
DEFINE TABLE IF NOT EXISTS `task` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `task` TYPE string;
DEFINE FIELD IF NOT EXISTS title ON `task` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS description ON `task` TYPE string;
DEFINE FIELD IF NOT EXISTS status ON `task` TYPE string;
DEFINE FIELD IF NOT EXISTS priority ON `task` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS assigned_to ON `task` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `task` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `task` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS started_at ON `task` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS completed_at ON `task` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS tags ON `task` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS acceptance_criteria ON `task` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS affected_files ON `task` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS estimated_complexity ON `task` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS actual_complexity ON `task` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS plan_id ON `task` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_task_id ON `task` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_task_status ON `task` FIELDS status;
DEFINE INDEX IF NOT EXISTS idx_task_plan ON `task` FIELDS plan_id;

-- ================================================================
-- Step
-- ================================================================
DEFINE TABLE IF NOT EXISTS `step` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `step` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `step` TYPE string;
DEFINE FIELD IF NOT EXISTS status ON `step` TYPE string;
DEFINE FIELD IF NOT EXISTS order_idx ON `step` TYPE int;
DEFINE FIELD IF NOT EXISTS verification ON `step` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS task_id ON `step` TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON `step` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `step` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS completed_at ON `step` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_step_id ON `step` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_step_task ON `step` FIELDS task_id;
DEFINE INDEX IF NOT EXISTS idx_step_status ON `step` FIELDS status;

-- ================================================================
-- Decision
-- ================================================================
DEFINE TABLE IF NOT EXISTS `decision` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `decision` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `decision` TYPE string;
DEFINE FIELD IF NOT EXISTS rationale ON `decision` TYPE string;
DEFINE FIELD IF NOT EXISTS alternatives ON `decision` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS chosen_option ON `decision` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS decided_by ON `decision` TYPE string;
DEFINE FIELD IF NOT EXISTS decided_at ON `decision` TYPE string;
DEFINE FIELD IF NOT EXISTS status ON `decision` TYPE string;
DEFINE FIELD IF NOT EXISTS task_id ON `decision` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS embedding ON `decision` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS embedding_model ON `decision` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_decision_id ON `decision` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_decision_status ON `decision` FIELDS status;
DEFINE INDEX IF NOT EXISTS idx_decision_task ON `decision` FIELDS task_id;

-- ================================================================
-- Constraint
-- ================================================================
DEFINE TABLE IF NOT EXISTS `constraint` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `constraint` TYPE string;
DEFINE FIELD IF NOT EXISTS constraint_type ON `constraint` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `constraint` TYPE string;
DEFINE FIELD IF NOT EXISTS enforced_by ON `constraint` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS plan_id ON `constraint` TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON `constraint` TYPE string;
DEFINE INDEX IF NOT EXISTS idx_constraint_id ON `constraint` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_constraint_plan ON `constraint` FIELDS plan_id;
DEFINE INDEX IF NOT EXISTS idx_constraint_type ON `constraint` FIELDS constraint_type;

-- ================================================================
-- Commit
-- ================================================================
DEFINE TABLE IF NOT EXISTS `commit` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS hash ON `commit` TYPE string;
DEFINE FIELD IF NOT EXISTS message ON `commit` TYPE string;
DEFINE FIELD IF NOT EXISTS author ON `commit` TYPE string;
DEFINE FIELD IF NOT EXISTS timestamp ON `commit` TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON `commit` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_commit_hash ON `commit` FIELDS hash UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_commit_project ON `commit` FIELDS project_id;

-- ================================================================
-- Release
-- ================================================================
DEFINE TABLE IF NOT EXISTS `release` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `release` TYPE string;
DEFINE FIELD IF NOT EXISTS version ON `release` TYPE string;
DEFINE FIELD IF NOT EXISTS title ON `release` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS description ON `release` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS status ON `release` TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON `release` TYPE string;
DEFINE FIELD IF NOT EXISTS target_date ON `release` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `release` TYPE string;
DEFINE FIELD IF NOT EXISTS released_at ON `release` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_release_id ON `release` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_release_project ON `release` FIELDS project_id;
DEFINE INDEX IF NOT EXISTS idx_release_version ON `release` FIELDS version;

-- ================================================================
-- Milestone
-- ================================================================
DEFINE TABLE IF NOT EXISTS `milestone` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `milestone` TYPE string;
DEFINE FIELD IF NOT EXISTS title ON `milestone` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `milestone` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS status ON `milestone` TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON `milestone` TYPE string;
DEFINE FIELD IF NOT EXISTS target_date ON `milestone` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS closed_at ON `milestone` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `milestone` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `milestone` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_milestone_id ON `milestone` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_milestone_project ON `milestone` FIELDS project_id;
DEFINE INDEX IF NOT EXISTS idx_milestone_status ON `milestone` FIELDS status;

-- ================================================================
-- Workspace Milestone
-- ================================================================
DEFINE TABLE IF NOT EXISTS `workspace_milestone` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `workspace_milestone` TYPE string;
DEFINE FIELD IF NOT EXISTS title ON `workspace_milestone` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `workspace_milestone` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS status ON `workspace_milestone` TYPE string;
DEFINE FIELD IF NOT EXISTS workspace_id ON `workspace_milestone` TYPE string;
DEFINE FIELD IF NOT EXISTS target_date ON `workspace_milestone` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS closed_at ON `workspace_milestone` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `workspace_milestone` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `workspace_milestone` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS tags ON `workspace_milestone` TYPE option<array>;
DEFINE INDEX IF NOT EXISTS idx_ws_milestone_id ON `workspace_milestone` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_ws_milestone_workspace ON `workspace_milestone` FIELDS workspace_id;
DEFINE INDEX IF NOT EXISTS idx_ws_milestone_status ON `workspace_milestone` FIELDS status;

-- ================================================================
-- Resource (shared API contracts)
-- ================================================================
DEFINE TABLE IF NOT EXISTS `resource` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `resource` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `resource` TYPE string;
DEFINE FIELD IF NOT EXISTS resource_type ON `resource` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `resource` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS url ON `resource` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS file_path ON `resource` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS format ON `resource` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS version ON `resource` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS workspace_id ON `resource` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS project_id ON `resource` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `resource` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `resource` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS metadata ON `resource` TYPE option<object> FLEXIBLE;
DEFINE INDEX IF NOT EXISTS idx_resource_id ON `resource` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_resource_workspace ON `resource` FIELDS workspace_id;
DEFINE INDEX IF NOT EXISTS idx_resource_type ON `resource` FIELDS resource_type;

-- ================================================================
-- Component (service topology)
-- ================================================================
DEFINE TABLE IF NOT EXISTS `component` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `component` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `component` TYPE string;
DEFINE FIELD IF NOT EXISTS component_type ON `component` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `component` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS workspace_id ON `component` TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON `component` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS runtime ON `component` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS config ON `component` TYPE option<object> FLEXIBLE;
DEFINE FIELD IF NOT EXISTS tags ON `component` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS created_at ON `component` TYPE string;
DEFINE INDEX IF NOT EXISTS idx_component_id ON `component` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_component_workspace ON `component` FIELDS workspace_id;
DEFINE INDEX IF NOT EXISTS idx_component_type ON `component` FIELDS component_type;

-- ================================================================
-- Note (knowledge)
-- ================================================================
DEFINE TABLE IF NOT EXISTS `note` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `note` TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS note_type ON `note` TYPE string;
DEFINE FIELD IF NOT EXISTS status ON `note` TYPE string;
DEFINE FIELD IF NOT EXISTS importance ON `note` TYPE string;
DEFINE FIELD IF NOT EXISTS content ON `note` TYPE string;
DEFINE FIELD IF NOT EXISTS tags ON `note` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS scope_type ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS scope_path ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS staleness_score ON `note` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS energy ON `note` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS code_anchor_hash ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `note` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS confirmed_at ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS embedding ON `note` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS embedding_model ON `note` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS valid_at ON `note` TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS invalid_at ON `note` TYPE option<datetime>;
DEFINE INDEX IF NOT EXISTS idx_note_id ON `note` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_note_project ON `note` FIELDS project_id;
DEFINE INDEX IF NOT EXISTS idx_note_status ON `note` FIELDS status;
DEFINE INDEX IF NOT EXISTS idx_note_type ON `note` FIELDS note_type;
DEFINE INDEX IF NOT EXISTS idx_note_importance ON `note` FIELDS importance;
DEFINE INDEX IF NOT EXISTS idx_note_staleness ON `note` FIELDS staleness_score;

-- ================================================================
-- Chat Session
-- ================================================================
DEFINE TABLE IF NOT EXISTS `chat_session` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `chat_session` TYPE string;
DEFINE FIELD IF NOT EXISTS cli_session_id ON `chat_session` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS project_slug ON `chat_session` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS workspace_slug ON `chat_session` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS cwd ON `chat_session` TYPE string;
DEFINE FIELD IF NOT EXISTS title ON `chat_session` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS model ON `chat_session` TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON `chat_session` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `chat_session` TYPE string;
DEFINE FIELD IF NOT EXISTS message_count ON `chat_session` TYPE int;
DEFINE FIELD IF NOT EXISTS total_cost_usd ON `chat_session` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS conversation_id ON `chat_session` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS preview ON `chat_session` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS permission_mode ON `chat_session` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS add_dirs ON `chat_session` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS auto_continue ON `chat_session` TYPE option<bool>;
DEFINE INDEX IF NOT EXISTS idx_chat_session_id ON `chat_session` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_chat_session_project ON `chat_session` FIELDS project_slug;
DEFINE INDEX IF NOT EXISTS idx_chat_session_cli ON `chat_session` FIELDS cli_session_id;

-- ================================================================
-- Chat Event
-- ================================================================
DEFINE TABLE IF NOT EXISTS `chat_event` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `chat_event` TYPE string;
DEFINE FIELD IF NOT EXISTS session_id ON `chat_event` TYPE string;
DEFINE FIELD IF NOT EXISTS seq ON `chat_event` TYPE int;
DEFINE FIELD IF NOT EXISTS event_type ON `chat_event` TYPE string;
DEFINE FIELD IF NOT EXISTS data ON `chat_event` TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON `chat_event` TYPE string;
DEFINE INDEX IF NOT EXISTS idx_chat_event_id ON `chat_event` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_chat_event_session ON `chat_event` FIELDS session_id;
DEFINE INDEX IF NOT EXISTS idx_chat_event_seq ON `chat_event` FIELDS session_id, seq;

-- ================================================================
-- Skill (neural)
-- ================================================================
DEFINE TABLE IF NOT EXISTS `skill` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `skill` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `skill` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `skill` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS status ON `skill` TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON `skill` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS energy ON `skill` TYPE float;
DEFINE FIELD IF NOT EXISTS cohesion ON `skill` TYPE float;
DEFINE FIELD IF NOT EXISTS activation_count ON `skill` TYPE int;
DEFINE FIELD IF NOT EXISTS centroid_embedding ON `skill` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS tags ON `skill` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS triggers ON `skill` TYPE option<array>;
DEFINE FIELD IF NOT EXISTS content_template ON `skill` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `skill` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `skill` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS last_activated_at ON `skill` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_skill_id ON `skill` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_skill_project ON `skill` FIELDS project_id;
DEFINE INDEX IF NOT EXISTS idx_skill_status ON `skill` FIELDS status;
DEFINE INDEX IF NOT EXISTS idx_skill_energy ON `skill` FIELDS energy;

-- ================================================================
-- Context Card (pre-computed analytics)
-- ================================================================
DEFINE TABLE IF NOT EXISTS `context_card` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS path ON `context_card` TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON `context_card` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS cc_pagerank ON `context_card` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS cc_betweenness ON `context_card` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS cc_clustering ON `context_card` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS cc_community_id ON `context_card` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS cc_community_label ON `context_card` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS cc_imports_out ON `context_card` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS cc_imports_in ON `context_card` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS cc_calls_out ON `context_card` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS cc_calls_in ON `context_card` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS cc_structural_dna ON `context_card` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS cc_wl_hash ON `context_card` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS cc_fingerprint ON `context_card` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS cc_co_changers_top5 ON `context_card` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS cc_version ON `context_card` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS cc_computed_at ON `context_card` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_context_card_path ON `context_card` FIELDS path;
DEFINE INDEX IF NOT EXISTS idx_context_card_project ON `context_card` FIELDS project_id;

-- ================================================================
-- Feature Graph
-- ================================================================
DEFINE TABLE IF NOT EXISTS `feature_graph` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `feature_graph` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `feature_graph` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `feature_graph` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS project_id ON `feature_graph` TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON `feature_graph` TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON `feature_graph` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS entry_function ON `feature_graph` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS build_depth ON `feature_graph` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS include_relations ON `feature_graph` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_feature_graph_id ON `feature_graph` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_feature_graph_project ON `feature_graph` FIELDS project_id;

-- ================================================================
-- User (auth)
-- ================================================================
DEFINE TABLE IF NOT EXISTS `user` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `user` TYPE string;
DEFINE FIELD IF NOT EXISTS email ON `user` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `user` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS password_hash ON `user` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS auth_provider ON `user` TYPE string;
DEFINE FIELD IF NOT EXISTS external_id ON `user` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS picture ON `user` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `user` TYPE string;
DEFINE FIELD IF NOT EXISTS last_login ON `user` TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_user_id ON `user` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_user_email ON `user` FIELDS email;
DEFINE INDEX IF NOT EXISTS idx_user_external ON `user` FIELDS external_id;

-- ================================================================
-- Refresh Token
-- ================================================================
DEFINE TABLE IF NOT EXISTS `refresh_token` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS token_hash ON `refresh_token` TYPE string;
DEFINE FIELD IF NOT EXISTS user_id ON `refresh_token` TYPE string;
DEFINE FIELD IF NOT EXISTS expires_at ON `refresh_token` TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON `refresh_token` TYPE string;
DEFINE FIELD IF NOT EXISTS revoked ON `refresh_token` TYPE option<bool>;
DEFINE INDEX IF NOT EXISTS idx_refresh_token_hash ON `refresh_token` FIELDS token_hash UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_refresh_token_user ON `refresh_token` FIELDS user_id;

-- ================================================================
-- Topology Rule
-- ================================================================
DEFINE TABLE IF NOT EXISTS `topology_rule` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `topology_rule` TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON `topology_rule` TYPE string;
DEFINE FIELD IF NOT EXISTS rule_type ON `topology_rule` TYPE string;
DEFINE FIELD IF NOT EXISTS source_pattern ON `topology_rule` TYPE string;
DEFINE FIELD IF NOT EXISTS target_pattern ON `topology_rule` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS max_value ON `topology_rule` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS description ON `topology_rule` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON `topology_rule` TYPE string;
DEFINE INDEX IF NOT EXISTS idx_topology_rule_id ON `topology_rule` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_topology_rule_project ON `topology_rule` FIELDS project_id;

-- ================================================================
-- Analysis Profile
-- ================================================================
DEFINE TABLE IF NOT EXISTS `analysis_profile` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `analysis_profile` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `analysis_profile` TYPE string;
DEFINE FIELD IF NOT EXISTS description ON `analysis_profile` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS project_id ON `analysis_profile` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS edge_weights ON `analysis_profile` TYPE option<object> FLEXIBLE;
DEFINE FIELD IF NOT EXISTS fusion_weights ON `analysis_profile` TYPE option<object> FLEXIBLE;
DEFINE FIELD IF NOT EXISTS created_at ON `analysis_profile` TYPE string;
DEFINE INDEX IF NOT EXISTS idx_analysis_profile_id ON `analysis_profile` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_analysis_profile_project ON `analysis_profile` FIELDS project_id;

-- ================================================================
-- Process (code clusters)
-- ================================================================
DEFINE TABLE IF NOT EXISTS `process` SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id ON `process` TYPE string;
DEFINE FIELD IF NOT EXISTS name ON `process` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS project_id ON `process` TYPE string;
DEFINE FIELD IF NOT EXISTS entry_point ON `process` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS file_count ON `process` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS function_count ON `process` TYPE option<int>;
DEFINE INDEX IF NOT EXISTS idx_process_id ON `process` FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS idx_process_project ON `process` FIELDS project_id;
"#,
            )
            .await
            .context("Failed to initialize node tables")?;
        Ok(())
    }

    /// Define all edge (relationship) tables.
    async fn init_edge_tables(&self) -> Result<()> {
        self.db
            .query(
                r#"
-- ================================================================
-- Code Structure Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `contains` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `contains` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `contains` TYPE record;

DEFINE TABLE IF NOT EXISTS `imports` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `imports` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `imports` TYPE record;
DEFINE FIELD IF NOT EXISTS import_path ON `imports` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS resolved ON `imports` TYPE option<bool>;

DEFINE TABLE IF NOT EXISTS `imports_symbol` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `imports_symbol` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `imports_symbol` TYPE record;
DEFINE FIELD IF NOT EXISTS resolved ON `imports_symbol` TYPE option<bool>;

DEFINE TABLE IF NOT EXISTS `calls` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `calls` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `calls` TYPE record;
DEFINE FIELD IF NOT EXISTS confidence ON `calls` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS reason ON `calls` TYPE option<string>;

DEFINE TABLE IF NOT EXISTS `extends` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `extends` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `extends` TYPE record;

DEFINE TABLE IF NOT EXISTS `implements` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `implements` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `implements` TYPE record;

DEFINE TABLE IF NOT EXISTS `implements_for` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `implements_for` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `implements_for` TYPE record;

DEFINE TABLE IF NOT EXISTS `uses_type` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `uses_type` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `uses_type` TYPE record;

-- ================================================================
-- Planning Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `has_plan` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_plan` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_plan` TYPE record;

DEFINE TABLE IF NOT EXISTS `has_task` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_task` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_task` TYPE record;

DEFINE TABLE IF NOT EXISTS `has_step` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_step` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_step` TYPE record;

DEFINE TABLE IF NOT EXISTS `depends_on` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `depends_on` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `depends_on` TYPE record;

DEFINE TABLE IF NOT EXISTS `constrained_by` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `constrained_by` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `constrained_by` TYPE record;

DEFINE TABLE IF NOT EXISTS `informed_by` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `informed_by` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `informed_by` TYPE record;

DEFINE TABLE IF NOT EXISTS `affects` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `affects` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `affects` TYPE record;
DEFINE FIELD IF NOT EXISTS entity_type ON `affects` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS entity_id ON `affects` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS entity_name ON `affects` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS impact_description ON `affects` TYPE option<string>;

DEFINE TABLE IF NOT EXISTS `resolved_by` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `resolved_by` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `resolved_by` TYPE record;

DEFINE TABLE IF NOT EXISTS `resulted_in` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `resulted_in` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `resulted_in` TYPE record;

-- ================================================================
-- Release & Milestone Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `has_release` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_release` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_release` TYPE record;

DEFINE TABLE IF NOT EXISTS `has_milestone` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_milestone` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_milestone` TYPE record;

DEFINE TABLE IF NOT EXISTS `includes_task` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `includes_task` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `includes_task` TYPE record;

DEFINE TABLE IF NOT EXISTS `includes_commit` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `includes_commit` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `includes_commit` TYPE record;

-- ================================================================
-- Knowledge Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `attached_to` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `attached_to` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `attached_to` TYPE record;
DEFINE FIELD IF NOT EXISTS anchor_type ON `attached_to` TYPE option<string>;

DEFINE TABLE IF NOT EXISTS `supersedes` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `supersedes` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `supersedes` TYPE record;

DEFINE TABLE IF NOT EXISTS `derived_from` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `derived_from` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `derived_from` TYPE record;

DEFINE TABLE IF NOT EXISTS `synapse` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `synapse` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `synapse` TYPE record;
DEFINE FIELD IF NOT EXISTS weight ON `synapse` TYPE float;

-- ================================================================
-- Knowledge Fabric Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `touches` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `touches` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `touches` TYPE record;
DEFINE FIELD IF NOT EXISTS additions ON `touches` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS deletions ON `touches` TYPE option<int>;

DEFINE TABLE IF NOT EXISTS `co_changed` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `co_changed` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `co_changed` TYPE record;
DEFINE FIELD IF NOT EXISTS weight ON `co_changed` TYPE float;
DEFINE FIELD IF NOT EXISTS count ON `co_changed` TYPE int;
DEFINE FIELD IF NOT EXISTS project_id ON `co_changed` TYPE option<string>;

DEFINE TABLE IF NOT EXISTS `discussed` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `discussed` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `discussed` TYPE record;
DEFINE FIELD IF NOT EXISTS mention_count ON `discussed` TYPE option<int>;
DEFINE FIELD IF NOT EXISTS last_mentioned_at ON `discussed` TYPE option<string>;

-- ================================================================
-- Workspace Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `belongs_to_workspace` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `belongs_to_workspace` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `belongs_to_workspace` TYPE record;

DEFINE TABLE IF NOT EXISTS `has_workspace_milestone` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_workspace_milestone` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_workspace_milestone` TYPE record;

DEFINE TABLE IF NOT EXISTS `has_resource` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_resource` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_resource` TYPE record;

DEFINE TABLE IF NOT EXISTS `implements_resource` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `implements_resource` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `implements_resource` TYPE record;

DEFINE TABLE IF NOT EXISTS `uses_resource` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `uses_resource` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `uses_resource` TYPE record;

DEFINE TABLE IF NOT EXISTS `has_component` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_component` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_component` TYPE record;

DEFINE TABLE IF NOT EXISTS `maps_to_project` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `maps_to_project` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `maps_to_project` TYPE record;

DEFINE TABLE IF NOT EXISTS `depends_on_component` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `depends_on_component` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `depends_on_component` TYPE record;
DEFINE FIELD IF NOT EXISTS protocol ON `depends_on_component` TYPE option<string>;
DEFINE FIELD IF NOT EXISTS required ON `depends_on_component` TYPE option<bool>;

-- ================================================================
-- Chat Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `has_chat_session` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_chat_session` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_chat_session` TYPE record;

DEFINE TABLE IF NOT EXISTS `has_event` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_event` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_event` TYPE record;

-- ================================================================
-- Skill Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `has_member` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `has_member` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `has_member` TYPE record;
DEFINE FIELD IF NOT EXISTS entity_type ON `has_member` TYPE option<string>;

-- ================================================================
-- Feature Graph Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `part_of_feature` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `part_of_feature` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `part_of_feature` TYPE record;
DEFINE FIELD IF NOT EXISTS role ON `part_of_feature` TYPE option<string>;

-- ================================================================
-- Analytics Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `predicted_link` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `predicted_link` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `predicted_link` TYPE record;
DEFINE FIELD IF NOT EXISTS plausibility ON `predicted_link` TYPE option<float>;
DEFINE FIELD IF NOT EXISTS project_id ON `predicted_link` TYPE option<string>;

DEFINE TABLE IF NOT EXISTS `step_in_process` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `step_in_process` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `step_in_process` TYPE record;

-- ================================================================
-- Task → File Edges
-- ================================================================
DEFINE TABLE IF NOT EXISTS `modifies_file` TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS in ON `modifies_file` TYPE record;
DEFINE FIELD IF NOT EXISTS out ON `modifies_file` TYPE record;
"#,
            )
            .await
            .context("Failed to initialize edge tables")?;
        Ok(())
    }

    /// Define vector indexes for semantic search.
    ///
    /// MTREE indexes may not be supported in the in-memory engine.
    /// Errors are logged but do not prevent startup.
    async fn init_vector_indexes(&self) -> Result<()> {
        let result = self.db
            .query(
                r#"
DEFINE INDEX IF NOT EXISTS idx_note_embedding ON `note` FIELDS embedding MTREE DIMENSION 768 DIST COSINE TYPE F32;
DEFINE INDEX IF NOT EXISTS idx_file_embedding ON `file` FIELDS embedding MTREE DIMENSION 768 DIST COSINE TYPE F32;
DEFINE INDEX IF NOT EXISTS idx_function_embedding ON `function` FIELDS embedding MTREE DIMENSION 768 DIST COSINE TYPE F32;
DEFINE INDEX IF NOT EXISTS idx_decision_embedding ON `decision` FIELDS embedding MTREE DIMENSION 768 DIST COSINE TYPE F32;
"#,
            )
            .await;

        if let Err(e) = result {
            tracing::warn!("Vector indexes not created (may be unsupported in this engine): {e}");
        }
        Ok(())
    }

    /// Define BM25 full-text search analyzer and indexes.
    ///
    /// Uses SurrealDB's native BM25 implementation with English snowball stemming.
    /// These indexes are required for the `@@` operator in FTS queries.
    ///
    /// Silently skips creation if the engine does not support BM25 (e.g. kv-mem
    /// in tests) — FTS methods fall back to CONTAINS-based keyword search.
    async fn init_fts_indexes(&self) -> Result<()> {
        let result = self
            .db
            .query(
                r#"
-- ====================================================================
-- Full-text search: BM25 analyzer and indexes
-- ====================================================================
DEFINE ANALYZER IF NOT EXISTS cortex_analyzer
    TOKENIZER class
    FILTERS lowercase, snowball(english);

DEFINE INDEX IF NOT EXISTS idx_note_fts
    ON note FIELDS content, tags
    SEARCH ANALYZER cortex_analyzer BM25(1.2, 0.75) HIGHLIGHTS;

DEFINE INDEX IF NOT EXISTS idx_decision_fts
    ON decision FIELDS description, rationale
    SEARCH ANALYZER cortex_analyzer BM25(1.2, 0.75) HIGHLIGHTS;

DEFINE INDEX IF NOT EXISTS idx_function_fts
    ON function FIELDS name, docstring
    SEARCH ANALYZER cortex_analyzer BM25(1.2, 0.75);

DEFINE INDEX IF NOT EXISTS idx_struct_fts
    ON `struct` FIELDS name, docstring
    SEARCH ANALYZER cortex_analyzer BM25(1.2, 0.75);

DEFINE INDEX IF NOT EXISTS idx_file_fts
    ON file FIELDS path
    SEARCH ANALYZER cortex_analyzer BM25(1.2, 0.75);

DEFINE INDEX IF NOT EXISTS idx_episode_fts
    ON episode FIELDS content, name
    SEARCH ANALYZER cortex_analyzer BM25(1.2, 0.75) HIGHLIGHTS;
"#,
            )
            .await;

        if let Err(e) = result {
            tracing::warn!("BM25 FTS indexes not created (may be unsupported in this engine): {e}");
        }
        Ok(())
    }

    /// Define the episode table schema.
    ///
    /// Creates the `episode` table with all required fields for episodic memory.
    /// Idempotent — safe to run multiple times.
    async fn init_episode_schema(&self) -> Result<()> {
        self.db
            .query(
                r#"
-- ================================================================
-- Episode (Graphiti-inspired temporal episodic memory)
-- ================================================================
DEFINE TABLE IF NOT EXISTS episode SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS name ON episode TYPE string;
DEFINE FIELD IF NOT EXISTS content ON episode TYPE string;
DEFINE FIELD IF NOT EXISTS source ON episode TYPE string;
DEFINE FIELD IF NOT EXISTS reference_time ON episode TYPE string;
DEFINE FIELD IF NOT EXISTS ingested_at ON episode TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON episode TYPE option<string>;
DEFINE FIELD IF NOT EXISTS group_id ON episode TYPE option<string>;
DEFINE INDEX IF NOT EXISTS idx_episode_project ON episode FIELDS project_id;
DEFINE INDEX IF NOT EXISTS idx_episode_group ON episode FIELDS group_id;
DEFINE INDEX IF NOT EXISTS idx_episode_ref_time ON episode FIELDS reference_time;
"#,
            )
            .await
            .context("Failed to initialize episode schema")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::client::IndentiaGraphStore;
    use surrealdb::types::{RecordId, SurrealValue};

    #[derive(Debug, SurrealValue)]
    struct TestRecord {
        id: RecordId,
    }

    #[tokio::test]
    async fn test_schema_init_creates_tables() {
        let store = IndentiaGraphStore::new_memory().await.unwrap();

        // Verify project table exists by inserting a record and reading it back
        store
            .db
            .query("CREATE project:test SET name = 'Test', slug = 'test', root_path = '/tmp', created_at = '2024-01-01T00:00:00Z' RETURN NONE")
            .await
            .unwrap();

        let mut response = store.db.query("SELECT * FROM project:test").await.unwrap();
        let result: Vec<TestRecord> = response.take(0).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn test_schema_init_is_idempotent() {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        assert!(store.init_schema().await.is_ok());
        assert!(store.init_schema().await.is_ok());
    }

    #[tokio::test]
    async fn test_unique_indexes_enforced() {
        let store = IndentiaGraphStore::new_memory().await.unwrap();

        // Create first project
        store
            .db
            .query("CREATE project:p1 SET name = 'Project 1', slug = 'proj-1', root_path = '/tmp/1', created_at = '2024-01-01T00:00:00Z' RETURN NONE")
            .await
            .unwrap();

        // Create second project with same slug — check if unique index catches it
        let result = store
            .db
            .query("CREATE project:p2 SET name = 'Project 2', slug = 'proj-1', root_path = '/tmp/2', created_at = '2024-01-01T00:00:00Z' RETURN NONE")
            .await;

        // SurrealDB may error on the query itself, or return the error in the response
        assert!(
            result.is_err() || {
                let mut response = result.unwrap();
                let check: Result<Vec<TestRecord>, _> = response.take(0);
                check.is_err()
            }
        );
    }

    #[tokio::test]
    async fn test_edge_tables_exist() {
        let store = IndentiaGraphStore::new_memory().await.unwrap();

        // Create nodes and an edge
        store
            .db
            .query(r#"
                CREATE project:p1 SET name = 'P1', slug = 'p1', root_path = '/tmp/p1', created_at = '2024-01-01T00:00:00Z' RETURN NONE;
                CREATE workspace:w1 SET name = 'W1', slug = 'w1', created_at = '2024-01-01T00:00:00Z' RETURN NONE;
                RELATE project:p1->belongs_to_workspace->workspace:w1 RETURN NONE;
            "#)
            .await
            .unwrap();

        // Verify the edge exists by counting edges from this source
        let mut response = store
            .db
            .query("SELECT count() AS total FROM belongs_to_workspace GROUP ALL")
            .await
            .unwrap();
        let result: Vec<surrealdb::types::Value> = response.take(0).unwrap();
        // At least one edge should exist
        assert!(!result.is_empty());
    }
}
