//! GraphStore trait implementation for IndentiaGraphStore.
//!
//! Delegates to domain-specific modules. Methods not yet implemented
//! return `todo!()` and will be filled in as each phase is completed.

#![allow(unused_variables)]

use crate::client::IndentiaGraphStore;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cortex_core::graph::{
    AnalysisProfile, FabricFileAnalyticsUpdate, FileAnalyticsUpdate, FunctionAnalyticsUpdate,
    TopologyRule, TopologyViolation,
};
use cortex_core::models::*;
use cortex_core::notes::{
    EntityType, Note, NoteAnchor, NoteFilters, NoteImportance, NoteStatus, PropagatedNote,
};
use cortex_core::parser_types::FunctionCall;
use cortex_core::plan::{TaskDetails, UpdateTaskRequest};
use cortex_core::skills::{ActivatedSkillContext, SkillNode, SkillStatus};
use cortex_graph::GraphStore;
use uuid::Uuid;

#[async_trait]
impl GraphStore for IndentiaGraphStore {
    async fn create_project(&self, project: &ProjectNode) -> Result<()> {
        self.create_project(project).await
    }

    async fn get_project(&self, id: Uuid) -> Result<Option<ProjectNode>> {
        self.get_project(id).await
    }

    async fn get_project_by_slug(&self, slug: &str) -> Result<Option<ProjectNode>> {
        self.get_project_by_slug(slug).await
    }

    async fn list_projects(&self) -> Result<Vec<ProjectNode>> {
        self.list_projects().await
    }

    async fn update_project(
        &self,
        id: Uuid,
        name: Option<String>,
        description: Option<Option<String>>,
        root_path: Option<String>,
    ) -> Result<()> {
        self.update_project(id, name, description, root_path).await
    }

    async fn update_project_synced(&self, id: Uuid) -> Result<()> {
        self.update_project_synced(id).await
    }

    async fn update_project_analytics_timestamp(&self, id: Uuid) -> Result<()> {
        self.update_project_analytics_timestamp(id).await
    }

    async fn delete_project(&self, id: Uuid, project_name: &str) -> Result<()> {
        self.delete_project(id, project_name).await
    }

    async fn create_workspace(&self, workspace: &WorkspaceNode) -> Result<()> {
        self.create_workspace(workspace).await
    }

    async fn get_workspace(&self, id: Uuid) -> Result<Option<WorkspaceNode>> {
        self.get_workspace(id).await
    }

    async fn get_workspace_by_slug(&self, slug: &str) -> Result<Option<WorkspaceNode>> {
        self.get_workspace_by_slug(slug).await
    }

    async fn list_workspaces(&self) -> Result<Vec<WorkspaceNode>> {
        self.list_workspaces().await
    }

    async fn update_workspace(
        &self,
        id: Uuid,
        name: Option<String>,
        description: Option<String>,
        metadata: Option<serde_json::Value>,
    ) -> Result<()> {
        self.update_workspace(id, name, description, metadata).await
    }

    async fn delete_workspace(&self, id: Uuid) -> Result<()> {
        self.delete_workspace(id).await
    }

    async fn add_project_to_workspace(&self, workspace_id: Uuid, project_id: Uuid) -> Result<()> {
        self.add_project_to_workspace(workspace_id, project_id)
            .await
    }

    async fn remove_project_from_workspace(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
    ) -> Result<()> {
        self.remove_project_from_workspace(workspace_id, project_id)
            .await
    }

    async fn list_workspace_projects(&self, workspace_id: Uuid) -> Result<Vec<ProjectNode>> {
        self.list_workspace_projects(workspace_id).await
    }

    async fn get_project_workspace(&self, project_id: Uuid) -> Result<Option<WorkspaceNode>> {
        self.get_project_workspace(project_id).await
    }

    async fn create_workspace_milestone(&self, milestone: &WorkspaceMilestoneNode) -> Result<()> {
        self.create_workspace_milestone(milestone).await
    }

    async fn get_workspace_milestone(&self, id: Uuid) -> Result<Option<WorkspaceMilestoneNode>> {
        self.get_workspace_milestone(id).await
    }

    async fn list_workspace_milestones(
        &self,
        workspace_id: Uuid,
    ) -> Result<Vec<WorkspaceMilestoneNode>> {
        self.list_workspace_milestones(workspace_id).await
    }

    async fn list_workspace_milestones_filtered(
        &self,
        workspace_id: Uuid,
        status: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<WorkspaceMilestoneNode>, usize)> {
        self.list_workspace_milestones_filtered(workspace_id, status, limit, offset)
            .await
    }

    async fn list_all_workspace_milestones_filtered(
        &self,
        workspace_id: Option<Uuid>,
        status: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(WorkspaceMilestoneNode, String, String, String)>> {
        self.list_all_workspace_milestones_filtered(workspace_id, status, limit, offset)
            .await
    }

    async fn count_all_workspace_milestones(
        &self,
        workspace_id: Option<Uuid>,
        status: Option<&str>,
    ) -> Result<usize> {
        self.count_all_workspace_milestones(workspace_id, status)
            .await
    }

    async fn update_workspace_milestone(
        &self,
        id: Uuid,
        title: Option<String>,
        description: Option<String>,
        status: Option<MilestoneStatus>,
        target_date: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()> {
        self.update_workspace_milestone(id, title, description, status, target_date)
            .await
    }

    async fn delete_workspace_milestone(&self, id: Uuid) -> Result<()> {
        self.delete_workspace_milestone(id).await
    }

    async fn add_task_to_workspace_milestone(
        &self,
        milestone_id: Uuid,
        task_id: Uuid,
    ) -> Result<()> {
        self.add_task_to_workspace_milestone(milestone_id, task_id)
            .await
    }

    async fn remove_task_from_workspace_milestone(
        &self,
        milestone_id: Uuid,
        task_id: Uuid,
    ) -> Result<()> {
        self.remove_task_from_workspace_milestone(milestone_id, task_id)
            .await
    }

    async fn link_plan_to_workspace_milestone(
        &self,
        plan_id: Uuid,
        milestone_id: Uuid,
    ) -> Result<()> {
        self.link_plan_to_workspace_milestone(plan_id, milestone_id)
            .await
    }

    async fn unlink_plan_from_workspace_milestone(
        &self,
        plan_id: Uuid,
        milestone_id: Uuid,
    ) -> Result<()> {
        self.unlink_plan_from_workspace_milestone(plan_id, milestone_id)
            .await
    }

    async fn get_workspace_milestone_progress(
        &self,
        milestone_id: Uuid,
    ) -> Result<(u32, u32, u32, u32)> {
        self.get_workspace_milestone_progress(milestone_id).await
    }

    async fn get_workspace_milestone_tasks(&self, milestone_id: Uuid) -> Result<Vec<TaskWithPlan>> {
        self.get_workspace_milestone_tasks(milestone_id).await
    }

    async fn get_workspace_milestone_steps(
        &self,
        milestone_id: Uuid,
    ) -> Result<std::collections::HashMap<Uuid, Vec<StepNode>>> {
        self.get_workspace_milestone_steps(milestone_id).await
    }

    async fn create_resource(&self, resource: &ResourceNode) -> Result<()> {
        self.create_resource(resource).await
    }

    async fn get_resource(&self, id: Uuid) -> Result<Option<ResourceNode>> {
        self.get_resource(id).await
    }

    async fn list_workspace_resources(&self, workspace_id: Uuid) -> Result<Vec<ResourceNode>> {
        self.list_workspace_resources(workspace_id).await
    }

    async fn update_resource(
        &self,
        id: Uuid,
        name: Option<String>,
        file_path: Option<String>,
        url: Option<String>,
        version: Option<String>,
        description: Option<String>,
    ) -> Result<()> {
        self.update_resource(id, name, file_path, url, version, description)
            .await
    }

    async fn delete_resource(&self, id: Uuid) -> Result<()> {
        self.delete_resource(id).await
    }

    async fn link_project_implements_resource(
        &self,
        project_id: Uuid,
        resource_id: Uuid,
    ) -> Result<()> {
        self.link_project_implements_resource(project_id, resource_id)
            .await
    }

    async fn link_project_uses_resource(&self, project_id: Uuid, resource_id: Uuid) -> Result<()> {
        self.link_project_uses_resource(project_id, resource_id)
            .await
    }

    async fn get_resource_implementers(&self, resource_id: Uuid) -> Result<Vec<ProjectNode>> {
        self.get_resource_implementers(resource_id).await
    }

    async fn get_resource_consumers(&self, resource_id: Uuid) -> Result<Vec<ProjectNode>> {
        self.get_resource_consumers(resource_id).await
    }

    async fn create_component(&self, component: &ComponentNode) -> Result<()> {
        self.create_component(component).await
    }

    async fn get_component(&self, id: Uuid) -> Result<Option<ComponentNode>> {
        self.get_component(id).await
    }

    async fn list_components(&self, workspace_id: Uuid) -> Result<Vec<ComponentNode>> {
        self.list_components(workspace_id).await
    }

    async fn update_component(
        &self,
        id: Uuid,
        name: Option<String>,
        description: Option<String>,
        runtime: Option<String>,
        config: Option<serde_json::Value>,
        tags: Option<Vec<String>>,
    ) -> Result<()> {
        self.update_component(id, name, description, runtime, config, tags)
            .await
    }

    async fn delete_component(&self, id: Uuid) -> Result<()> {
        self.delete_component(id).await
    }

    async fn add_component_dependency(
        &self,
        component_id: Uuid,
        depends_on_id: Uuid,
        protocol: Option<String>,
        required: bool,
    ) -> Result<()> {
        self.add_component_dependency(component_id, depends_on_id, protocol, required)
            .await
    }

    async fn remove_component_dependency(
        &self,
        component_id: Uuid,
        depends_on_id: Uuid,
    ) -> Result<()> {
        self.remove_component_dependency(component_id, depends_on_id)
            .await
    }

    async fn map_component_to_project(&self, component_id: Uuid, project_id: Uuid) -> Result<()> {
        self.map_component_to_project(component_id, project_id)
            .await
    }

    async fn get_workspace_topology(
        &self,
        workspace_id: Uuid,
    ) -> Result<Vec<(ComponentNode, Option<String>, Vec<ComponentDependency>)>> {
        self.get_workspace_topology(workspace_id).await
    }

    async fn get_project_file_paths(&self, project_id: Uuid) -> Result<Vec<String>> {
        self.get_project_file_paths(project_id).await
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        self.delete_file(path).await
    }

    async fn delete_stale_files(
        &self,
        project_id: Uuid,
        valid_paths: &[String],
    ) -> Result<(usize, usize, Vec<String>)> {
        self.delete_stale_files(project_id, valid_paths).await
    }

    async fn link_file_to_project(&self, file_path: &str, project_id: Uuid) -> Result<()> {
        self.link_file_to_project(file_path, project_id).await
    }

    async fn upsert_file(&self, file: &FileNode) -> Result<()> {
        self.upsert_file(file).await
    }

    async fn batch_upsert_files(&self, files: &[FileNode]) -> Result<()> {
        self.batch_upsert_files(files).await
    }

    async fn get_file(&self, path: &str) -> Result<Option<FileNode>> {
        self.get_file(path).await
    }

    async fn list_project_files(&self, project_id: Uuid) -> Result<Vec<FileNode>> {
        self.list_project_files(project_id).await
    }

    async fn count_project_files(&self, project_id: Uuid) -> Result<i64> {
        self.count_project_files(project_id).await
    }

    async fn invalidate_computed_properties(
        &self,
        project_id: Uuid,
        paths: &[String],
    ) -> Result<u64> {
        self.invalidate_computed_properties(project_id, paths).await
    }

    async fn upsert_function(&self, func: &FunctionNode) -> Result<()> {
        self.upsert_function(func).await
    }

    async fn upsert_struct(&self, s: &StructNode) -> Result<()> {
        self.upsert_struct(s).await
    }

    async fn upsert_trait(&self, t: &TraitNode) -> Result<()> {
        self.upsert_trait(t).await
    }

    async fn find_trait_by_name(&self, name: &str) -> Result<Option<String>> {
        self.find_trait_by_name(name).await
    }

    async fn upsert_enum(&self, e: &EnumNode) -> Result<()> {
        self.upsert_enum(e).await
    }

    async fn upsert_impl(&self, impl_node: &ImplNode) -> Result<()> {
        self.upsert_impl(impl_node).await
    }

    async fn create_import_relationship(
        &self,
        from_file: &str,
        to_file: &str,
        import_path: &str,
    ) -> Result<()> {
        self.create_import_relationship(from_file, to_file, import_path)
            .await
    }

    async fn upsert_import(&self, import: &ImportNode) -> Result<()> {
        self.upsert_import(import).await
    }

    async fn create_imports_symbol_relationship(
        &self,
        import_id: &str,
        symbol_name: &str,
        project_id: Option<Uuid>,
    ) -> Result<()> {
        self.create_imports_symbol_relationship(import_id, symbol_name, project_id)
            .await
    }

    async fn create_call_relationship(
        &self,
        caller_id: &str,
        callee_name: &str,
        project_id: Option<Uuid>,
        confidence: f64,
        reason: &str,
    ) -> Result<()> {
        self.create_call_relationship(caller_id, callee_name, project_id, confidence, reason)
            .await
    }

    async fn batch_upsert_functions(&self, functions: &[FunctionNode]) -> Result<()> {
        self.batch_upsert_functions(functions).await
    }

    async fn batch_upsert_structs(&self, structs: &[StructNode]) -> Result<()> {
        self.batch_upsert_structs(structs).await
    }

    async fn batch_upsert_traits(&self, traits: &[TraitNode]) -> Result<()> {
        self.batch_upsert_traits(traits).await
    }

    async fn batch_upsert_enums(&self, enums: &[EnumNode]) -> Result<()> {
        self.batch_upsert_enums(enums).await
    }

    async fn batch_upsert_impls(&self, impls: &[ImplNode]) -> Result<()> {
        self.batch_upsert_impls(impls).await
    }

    async fn batch_upsert_imports(&self, imports: &[ImportNode]) -> Result<()> {
        self.batch_upsert_imports(imports).await
    }

    async fn batch_create_import_relationships(
        &self,
        relationships: &[(String, String, String)],
    ) -> Result<()> {
        self.batch_create_import_relationships(relationships).await
    }

    async fn batch_create_imports_symbol_relationships(
        &self,
        relationships: &[(String, String, Option<Uuid>)],
    ) -> Result<()> {
        self.batch_create_imports_symbol_relationships(relationships)
            .await
    }

    async fn batch_create_call_relationships(
        &self,
        calls: &[FunctionCall],
        project_id: Option<Uuid>,
    ) -> Result<()> {
        self.batch_create_call_relationships(calls, project_id)
            .await
    }

    async fn batch_create_extends_relationships(
        &self,
        rels: &[(String, String, String, String)],
    ) -> Result<()> {
        self.batch_create_extends_relationships(rels).await
    }

    async fn batch_create_implements_relationships(
        &self,
        rels: &[(String, String, String, String)],
    ) -> Result<()> {
        self.batch_create_implements_relationships(rels).await
    }

    async fn cleanup_cross_project_calls(&self) -> Result<i64> {
        self.cleanup_cross_project_calls().await
    }

    async fn cleanup_builtin_calls(&self) -> Result<i64> {
        self.cleanup_builtin_calls().await
    }

    async fn migrate_calls_confidence(&self) -> Result<i64> {
        self.migrate_calls_confidence().await
    }

    async fn cleanup_sync_data(&self) -> Result<i64> {
        self.cleanup_sync_data().await
    }

    async fn get_callees(&self, function_id: &str, depth: u32) -> Result<Vec<FunctionNode>> {
        self.get_callees(function_id, depth).await
    }

    async fn create_uses_type_relationship(
        &self,
        function_id: &str,
        type_name: &str,
    ) -> Result<()> {
        self.create_uses_type_relationship(function_id, type_name)
            .await
    }

    async fn find_trait_implementors(&self, trait_name: &str) -> Result<Vec<String>> {
        self.find_trait_implementors(trait_name).await
    }

    async fn get_type_traits(&self, type_name: &str) -> Result<Vec<String>> {
        self.get_type_traits(type_name).await
    }

    async fn get_impl_blocks(&self, type_name: &str) -> Result<Vec<serde_json::Value>> {
        self.get_impl_blocks(type_name).await
    }

    async fn get_class_hierarchy(
        &self,
        type_name: &str,
        max_depth: u32,
    ) -> Result<serde_json::Value> {
        self.get_class_hierarchy(type_name, max_depth).await
    }

    async fn find_subclasses(&self, class_name: &str) -> Result<Vec<serde_json::Value>> {
        self.find_subclasses(class_name).await
    }

    async fn find_interface_implementors(
        &self,
        interface_name: &str,
    ) -> Result<Vec<serde_json::Value>> {
        self.find_interface_implementors(interface_name).await
    }

    async fn list_processes(&self, project_id: uuid::Uuid) -> Result<Vec<serde_json::Value>> {
        self.list_processes(project_id).await
    }

    async fn get_process_detail(&self, process_id: &str) -> Result<Option<serde_json::Value>> {
        self.get_process_detail(process_id).await
    }

    async fn get_entry_points(
        &self,
        project_id: uuid::Uuid,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        self.get_entry_points(project_id, limit).await
    }

    async fn get_file_language(&self, path: &str) -> Result<Option<String>> {
        self.get_file_language(path).await
    }

    async fn get_file_functions_summary(&self, path: &str) -> Result<Vec<FunctionSummaryNode>> {
        self.get_file_functions_summary(path).await
    }

    async fn get_file_structs_summary(&self, path: &str) -> Result<Vec<StructSummaryNode>> {
        self.get_file_structs_summary(path).await
    }

    async fn get_file_import_paths_list(&self, path: &str) -> Result<Vec<String>> {
        self.get_file_import_paths_list(path).await
    }

    async fn find_symbol_references(
        &self,
        symbol: &str,
        limit: usize,
        project_id: Option<Uuid>,
    ) -> Result<Vec<SymbolReferenceNode>> {
        self.find_symbol_references(symbol, limit, project_id).await
    }

    async fn get_file_direct_imports(&self, path: &str) -> Result<Vec<FileImportNode>> {
        self.get_file_direct_imports(path).await
    }

    async fn get_function_callers_by_name(
        &self,
        function_name: &str,
        depth: u32,
        project_id: Option<Uuid>,
    ) -> Result<Vec<String>> {
        self.get_function_callers_by_name(function_name, depth, project_id)
            .await
    }

    async fn get_function_callees_by_name(
        &self,
        function_name: &str,
        depth: u32,
        project_id: Option<Uuid>,
    ) -> Result<Vec<String>> {
        self.get_function_callees_by_name(function_name, depth, project_id)
            .await
    }

    async fn get_callers_with_confidence(
        &self,
        function_name: &str,
        project_id: Option<Uuid>,
    ) -> Result<Vec<(String, String, f64, String)>> {
        self.get_callers_with_confidence(function_name, project_id)
            .await
    }

    async fn get_callees_with_confidence(
        &self,
        function_name: &str,
        project_id: Option<Uuid>,
    ) -> Result<Vec<(String, String, f64, String)>> {
        self.get_callees_with_confidence(function_name, project_id)
            .await
    }

    async fn get_language_stats(&self) -> Result<Vec<LanguageStatsNode>> {
        self.get_language_stats().await
    }

    async fn get_language_stats_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<LanguageStatsNode>> {
        self.get_language_stats_for_project(project_id).await
    }

    async fn get_most_connected_files(&self, limit: usize) -> Result<Vec<String>> {
        self.get_most_connected_files(limit).await
    }

    async fn get_most_connected_files_detailed(
        &self,
        limit: usize,
    ) -> Result<Vec<ConnectedFileNode>> {
        self.get_most_connected_files_detailed(limit).await
    }

    async fn get_most_connected_files_for_project(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> Result<Vec<ConnectedFileNode>> {
        self.get_most_connected_files_for_project(project_id, limit)
            .await
    }

    async fn get_project_communities(&self, project_id: Uuid) -> Result<Vec<CommunityRow>> {
        self.get_project_communities(project_id).await
    }

    async fn get_node_analytics(
        &self,
        identifier: &str,
        node_type: &str,
    ) -> Result<Option<NodeAnalyticsRow>> {
        self.get_node_analytics(identifier, node_type).await
    }

    async fn get_affected_communities(&self, file_paths: &[String]) -> Result<Vec<String>> {
        self.get_affected_communities(file_paths).await
    }

    async fn get_code_health_report(
        &self,
        project_id: Uuid,
        god_function_threshold: usize,
    ) -> Result<cortex_core::models::CodeHealthReport> {
        self.get_code_health_report(project_id, god_function_threshold)
            .await
    }

    async fn get_circular_dependencies(&self, project_id: Uuid) -> Result<Vec<Vec<String>>> {
        self.get_circular_dependencies(project_id).await
    }

    async fn get_node_gds_metrics(
        &self,
        node_path: &str,
        node_type: &str,
        project_id: Uuid,
    ) -> Result<Option<NodeGdsMetrics>> {
        self.get_node_gds_metrics(node_path, node_type, project_id)
            .await
    }

    async fn get_project_percentiles(&self, project_id: Uuid) -> Result<ProjectPercentiles> {
        self.get_project_percentiles(project_id).await
    }

    async fn get_top_bridges_by_betweenness(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> Result<Vec<BridgeFile>> {
        self.get_top_bridges_by_betweenness(project_id, limit).await
    }

    async fn get_file_symbol_names(&self, path: &str) -> Result<FileSymbolNamesNode> {
        self.get_file_symbol_names(path).await
    }

    async fn get_function_caller_count(
        &self,
        function_name: &str,
        project_id: Option<Uuid>,
    ) -> Result<i64> {
        self.get_function_caller_count(function_name, project_id)
            .await
    }

    async fn get_trait_info(&self, trait_name: &str) -> Result<Option<TraitInfoNode>> {
        self.get_trait_info(trait_name).await
    }

    async fn get_trait_implementors_detailed(
        &self,
        trait_name: &str,
    ) -> Result<Vec<TraitImplementorNode>> {
        self.get_trait_implementors_detailed(trait_name).await
    }

    async fn get_type_trait_implementations(
        &self,
        type_name: &str,
    ) -> Result<Vec<TypeTraitInfoNode>> {
        self.get_type_trait_implementations(type_name).await
    }

    async fn get_type_impl_blocks_detailed(
        &self,
        type_name: &str,
    ) -> Result<Vec<ImplBlockDetailNode>> {
        self.get_type_impl_blocks_detailed(type_name).await
    }

    async fn create_plan(&self, plan: &PlanNode) -> Result<()> {
        self.create_plan(plan).await
    }

    async fn get_plan(&self, id: Uuid) -> Result<Option<PlanNode>> {
        self.get_plan(id).await
    }

    async fn list_active_plans(&self) -> Result<Vec<PlanNode>> {
        self.list_active_plans().await
    }

    async fn list_project_plans(&self, project_id: Uuid) -> Result<Vec<PlanNode>> {
        self.list_project_plans(project_id).await
    }

    async fn count_project_plans(&self, project_id: Uuid) -> Result<i64> {
        self.count_project_plans(project_id).await
    }

    async fn list_plans_for_project(
        &self,
        project_id: Uuid,
        status_filter: Option<Vec<String>>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<PlanNode>, usize)> {
        self.list_plans_for_project(project_id, status_filter, limit, offset)
            .await
    }

    async fn update_plan_status(&self, id: Uuid, status: PlanStatus) -> Result<()> {
        self.update_plan_status(id, status).await
    }

    async fn link_plan_to_project(&self, plan_id: Uuid, project_id: Uuid) -> Result<()> {
        self.link_plan_to_project(plan_id, project_id).await
    }

    async fn unlink_plan_from_project(&self, plan_id: Uuid) -> Result<()> {
        self.unlink_plan_from_project(plan_id).await
    }

    async fn delete_plan(&self, plan_id: Uuid) -> Result<()> {
        self.delete_plan(plan_id).await
    }

    async fn create_task(&self, plan_id: Uuid, task: &TaskNode) -> Result<()> {
        self.create_task(plan_id, task).await
    }

    async fn get_plan_tasks(&self, plan_id: Uuid) -> Result<Vec<TaskNode>> {
        self.get_plan_tasks(plan_id).await
    }

    async fn get_task_with_full_details(&self, task_id: Uuid) -> Result<Option<TaskDetails>> {
        self.get_task_with_full_details(task_id).await
    }

    async fn analyze_task_impact(&self, task_id: Uuid) -> Result<Vec<String>> {
        self.analyze_task_impact(task_id).await
    }

    async fn find_blocked_tasks(&self, plan_id: Uuid) -> Result<Vec<(TaskNode, Vec<TaskNode>)>> {
        self.find_blocked_tasks(plan_id).await
    }

    async fn update_task_status(&self, task_id: Uuid, status: TaskStatus) -> Result<()> {
        self.update_task_status(task_id, status).await
    }

    async fn assign_task(&self, task_id: Uuid, agent_id: &str) -> Result<()> {
        self.assign_task(task_id, agent_id).await
    }

    async fn add_task_dependency(&self, task_id: Uuid, depends_on_id: Uuid) -> Result<()> {
        self.add_task_dependency(task_id, depends_on_id).await
    }

    async fn remove_task_dependency(&self, task_id: Uuid, depends_on_id: Uuid) -> Result<()> {
        self.remove_task_dependency(task_id, depends_on_id).await
    }

    async fn get_task_blockers(&self, task_id: Uuid) -> Result<Vec<TaskNode>> {
        self.get_task_blockers(task_id).await
    }

    async fn get_tasks_blocked_by(&self, task_id: Uuid) -> Result<Vec<TaskNode>> {
        self.get_tasks_blocked_by(task_id).await
    }

    async fn get_task_dependencies(&self, task_id: Uuid) -> Result<Vec<TaskNode>> {
        self.get_task_dependencies(task_id).await
    }

    async fn get_plan_dependency_graph(
        &self,
        plan_id: Uuid,
    ) -> Result<(Vec<TaskNode>, Vec<(Uuid, Uuid)>)> {
        self.get_plan_dependency_graph(plan_id).await
    }

    async fn get_plan_critical_path(&self, plan_id: Uuid) -> Result<Vec<TaskNode>> {
        self.get_plan_critical_path(plan_id).await
    }

    async fn get_next_available_task(&self, plan_id: Uuid) -> Result<Option<TaskNode>> {
        self.get_next_available_task(plan_id).await
    }

    async fn get_task(&self, task_id: Uuid) -> Result<Option<TaskNode>> {
        self.get_task(task_id).await
    }

    async fn update_task(&self, task_id: Uuid, updates: &UpdateTaskRequest) -> Result<()> {
        self.update_task(task_id, updates).await
    }

    async fn delete_task(&self, task_id: Uuid) -> Result<()> {
        self.delete_task(task_id).await
    }

    async fn get_project_for_task(&self, task_id: Uuid) -> Result<Option<ProjectNode>> {
        self.get_project_for_task(task_id).await
    }

    async fn create_step(&self, task_id: Uuid, step: &StepNode) -> Result<()> {
        self.create_step(task_id, step).await
    }

    async fn get_task_steps(&self, task_id: Uuid) -> Result<Vec<StepNode>> {
        self.get_task_steps(task_id).await
    }

    async fn update_step_status(&self, step_id: Uuid, status: StepStatus) -> Result<()> {
        self.update_step_status(step_id, status).await
    }

    async fn get_task_step_progress(&self, task_id: Uuid) -> Result<(u32, u32)> {
        self.get_task_step_progress(task_id).await
    }

    async fn get_step(&self, step_id: Uuid) -> Result<Option<StepNode>> {
        self.get_step(step_id).await
    }

    async fn delete_step(&self, step_id: Uuid) -> Result<()> {
        self.delete_step(step_id).await
    }

    async fn create_constraint(&self, plan_id: Uuid, constraint: &ConstraintNode) -> Result<()> {
        self.create_constraint(plan_id, constraint).await
    }

    async fn get_plan_constraints(&self, plan_id: Uuid) -> Result<Vec<ConstraintNode>> {
        self.get_plan_constraints(plan_id).await
    }

    async fn get_constraint(&self, constraint_id: Uuid) -> Result<Option<ConstraintNode>> {
        self.get_constraint(constraint_id).await
    }

    async fn update_constraint(
        &self,
        constraint_id: Uuid,
        description: Option<String>,
        constraint_type: Option<ConstraintType>,
        enforced_by: Option<String>,
    ) -> Result<()> {
        self.update_constraint(constraint_id, description, constraint_type, enforced_by)
            .await
    }

    async fn delete_constraint(&self, constraint_id: Uuid) -> Result<()> {
        self.delete_constraint(constraint_id).await
    }

    async fn create_decision(&self, task_id: Uuid, decision: &DecisionNode) -> Result<()> {
        self.create_decision(task_id, decision).await
    }

    async fn get_decision(&self, decision_id: Uuid) -> Result<Option<DecisionNode>> {
        self.get_decision(decision_id).await
    }

    async fn update_decision(
        &self,
        decision_id: Uuid,
        description: Option<String>,
        rationale: Option<String>,
        chosen_option: Option<String>,
        status: Option<DecisionStatus>,
    ) -> Result<()> {
        self.update_decision(decision_id, description, rationale, chosen_option, status)
            .await
    }

    async fn delete_decision(&self, decision_id: Uuid) -> Result<()> {
        self.delete_decision(decision_id).await
    }

    async fn get_decisions_for_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
        limit: u32,
    ) -> Result<Vec<DecisionNode>> {
        self.get_decisions_for_entity(entity_type, entity_id, limit)
            .await
    }

    async fn set_decision_embedding(
        &self,
        decision_id: Uuid,
        embedding: &[f32],
        model: &str,
    ) -> Result<()> {
        self.set_decision_embedding(decision_id, embedding, model)
            .await
    }

    async fn get_decision_embedding(&self, decision_id: Uuid) -> Result<Option<Vec<f32>>> {
        self.get_decision_embedding(decision_id).await
    }

    async fn get_all_decisions_with_task_id(&self) -> Result<Vec<(DecisionNode, Uuid)>> {
        self.get_all_decisions_with_task_id().await
    }

    async fn get_decisions_without_embedding(&self) -> Result<Vec<(Uuid, String, String)>> {
        self.get_decisions_without_embedding().await
    }

    async fn search_decisions_by_vector(
        &self,
        query_embedding: &[f32],
        limit: usize,
        project_id: Option<&str>,
    ) -> Result<Vec<(DecisionNode, f64)>> {
        self.search_decisions_by_vector(query_embedding, limit, project_id)
            .await
    }

    async fn get_decisions_affecting(
        &self,
        entity_type: &str,
        entity_id: &str,
        status_filter: Option<&str>,
    ) -> Result<Vec<DecisionNode>> {
        self.get_decisions_affecting(entity_type, entity_id, status_filter)
            .await
    }

    async fn add_decision_affects(
        &self,
        decision_id: Uuid,
        entity_type: &str,
        entity_id: &str,
        impact_description: Option<&str>,
    ) -> Result<()> {
        self.add_decision_affects(decision_id, entity_type, entity_id, impact_description)
            .await
    }

    async fn remove_decision_affects(
        &self,
        decision_id: Uuid,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<()> {
        self.remove_decision_affects(decision_id, entity_type, entity_id)
            .await
    }

    async fn list_decision_affects(&self, decision_id: Uuid) -> Result<Vec<AffectsRelation>> {
        self.list_decision_affects(decision_id).await
    }

    async fn supersede_decision(&self, new_decision_id: Uuid, old_decision_id: Uuid) -> Result<()> {
        self.supersede_decision(new_decision_id, old_decision_id)
            .await
    }

    async fn get_decision_timeline(
        &self,
        task_id: Option<Uuid>,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<Vec<DecisionTimelineEntry>> {
        self.get_decision_timeline(task_id, from, to).await
    }

    async fn find_dependent_files(
        &self,
        file_path: &str,
        depth: u32,
        project_id: Option<Uuid>,
    ) -> Result<Vec<String>> {
        self.find_dependent_files(file_path, depth, project_id)
            .await
    }

    async fn find_impacted_files(
        &self,
        file_path: &str,
        depth: u32,
        project_id: Option<Uuid>,
    ) -> Result<Vec<String>> {
        self.find_impacted_files(file_path, depth, project_id).await
    }

    async fn find_callers(
        &self,
        function_id: &str,
        project_id: Option<Uuid>,
    ) -> Result<Vec<FunctionNode>> {
        self.find_callers(function_id, project_id).await
    }

    async fn link_task_to_files(&self, task_id: Uuid, file_paths: &[String]) -> Result<()> {
        self.link_task_to_files(task_id, file_paths).await
    }

    async fn create_commit(&self, commit: &CommitNode) -> Result<()> {
        self.create_commit(commit).await
    }

    async fn get_commit(&self, hash: &str) -> Result<Option<CommitNode>> {
        self.get_commit(hash).await
    }

    async fn link_commit_to_task(&self, commit_hash: &str, task_id: Uuid) -> Result<()> {
        self.link_commit_to_task(commit_hash, task_id).await
    }

    async fn link_commit_to_plan(&self, commit_hash: &str, plan_id: Uuid) -> Result<()> {
        self.link_commit_to_plan(commit_hash, plan_id).await
    }

    async fn get_task_commits(&self, task_id: Uuid) -> Result<Vec<CommitNode>> {
        self.get_task_commits(task_id).await
    }

    async fn get_plan_commits(&self, plan_id: Uuid) -> Result<Vec<CommitNode>> {
        self.get_plan_commits(plan_id).await
    }

    async fn delete_commit(&self, hash: &str) -> Result<()> {
        self.delete_commit(hash).await
    }

    async fn create_commit_touches(
        &self,
        commit_hash: &str,
        files: &[FileChangedInfo],
    ) -> Result<()> {
        self.create_commit_touches(commit_hash, files).await
    }

    async fn get_commit_files(&self, commit_hash: &str) -> Result<Vec<CommitFileInfo>> {
        self.get_commit_files(commit_hash).await
    }

    async fn get_file_history(
        &self,
        file_path: &str,
        limit: Option<i64>,
    ) -> Result<Vec<FileHistoryEntry>> {
        self.get_file_history(file_path, limit).await
    }

    async fn compute_co_changed(
        &self,
        project_id: Uuid,
        since: Option<chrono::DateTime<chrono::Utc>>,
        min_count: i64,
        max_relations: i64,
    ) -> Result<i64> {
        self.compute_co_changed(project_id, since, min_count, max_relations)
            .await
    }

    async fn update_project_co_change_timestamp(&self, id: Uuid) -> Result<()> {
        self.update_project_co_change_timestamp(id).await
    }

    async fn get_co_change_graph(
        &self,
        project_id: Uuid,
        min_count: i64,
        limit: i64,
    ) -> Result<Vec<CoChangePair>> {
        self.get_co_change_graph(project_id, min_count, limit).await
    }

    async fn get_file_co_changers(
        &self,
        file_path: &str,
        min_count: i64,
        limit: i64,
    ) -> Result<Vec<CoChanger>> {
        self.get_file_co_changers(file_path, min_count, limit).await
    }

    async fn create_release(&self, release: &ReleaseNode) -> Result<()> {
        self.create_release(release).await
    }

    async fn get_release(&self, id: Uuid) -> Result<Option<ReleaseNode>> {
        self.get_release(id).await
    }

    async fn list_project_releases(&self, project_id: Uuid) -> Result<Vec<ReleaseNode>> {
        self.list_project_releases(project_id).await
    }

    async fn update_release(
        &self,
        id: Uuid,
        status: Option<ReleaseStatus>,
        target_date: Option<chrono::DateTime<chrono::Utc>>,
        released_at: Option<chrono::DateTime<chrono::Utc>>,
        title: Option<String>,
        description: Option<String>,
    ) -> Result<()> {
        self.update_release(id, status, target_date, released_at, title, description)
            .await
    }

    async fn add_task_to_release(&self, release_id: Uuid, task_id: Uuid) -> Result<()> {
        self.add_task_to_release(release_id, task_id).await
    }

    async fn add_commit_to_release(&self, release_id: Uuid, commit_hash: &str) -> Result<()> {
        self.add_commit_to_release(release_id, commit_hash).await
    }

    async fn remove_commit_from_release(&self, release_id: Uuid, commit_hash: &str) -> Result<()> {
        self.remove_commit_from_release(release_id, commit_hash)
            .await
    }

    async fn get_release_details(
        &self,
        release_id: Uuid,
    ) -> Result<Option<(ReleaseNode, Vec<TaskNode>, Vec<CommitNode>)>> {
        self.get_release_details(release_id).await
    }

    async fn delete_release(&self, release_id: Uuid) -> Result<()> {
        self.delete_release(release_id).await
    }

    async fn create_milestone(&self, milestone: &MilestoneNode) -> Result<()> {
        self.create_milestone(milestone).await
    }

    async fn get_milestone(&self, id: Uuid) -> Result<Option<MilestoneNode>> {
        self.get_milestone(id).await
    }

    async fn list_project_milestones(&self, project_id: Uuid) -> Result<Vec<MilestoneNode>> {
        self.list_project_milestones(project_id).await
    }

    async fn update_milestone(
        &self,
        id: Uuid,
        status: Option<MilestoneStatus>,
        target_date: Option<chrono::DateTime<chrono::Utc>>,
        closed_at: Option<chrono::DateTime<chrono::Utc>>,
        title: Option<String>,
        description: Option<String>,
    ) -> Result<()> {
        self.update_milestone(id, status, target_date, closed_at, title, description)
            .await
    }

    async fn add_task_to_milestone(&self, milestone_id: Uuid, task_id: Uuid) -> Result<()> {
        self.add_task_to_milestone(milestone_id, task_id).await
    }

    async fn link_plan_to_milestone(&self, plan_id: Uuid, milestone_id: Uuid) -> Result<()> {
        self.link_plan_to_milestone(plan_id, milestone_id).await
    }

    async fn unlink_plan_from_milestone(&self, plan_id: Uuid, milestone_id: Uuid) -> Result<()> {
        self.unlink_plan_from_milestone(plan_id, milestone_id).await
    }

    async fn get_milestone_details(
        &self,
        milestone_id: Uuid,
    ) -> Result<Option<(MilestoneNode, Vec<TaskNode>)>> {
        self.get_milestone_details(milestone_id).await
    }

    async fn get_milestone_progress(&self, milestone_id: Uuid) -> Result<(u32, u32, u32, u32)> {
        self.get_milestone_progress(milestone_id).await
    }

    async fn get_milestone_tasks_with_plans(
        &self,
        milestone_id: Uuid,
    ) -> Result<Vec<TaskWithPlan>> {
        self.get_milestone_tasks_with_plans(milestone_id).await
    }

    async fn get_milestone_steps_batch(
        &self,
        milestone_id: Uuid,
    ) -> Result<std::collections::HashMap<Uuid, Vec<StepNode>>> {
        self.get_milestone_steps_batch(milestone_id).await
    }

    async fn delete_milestone(&self, milestone_id: Uuid) -> Result<()> {
        self.delete_milestone(milestone_id).await
    }

    async fn get_milestone_tasks(&self, milestone_id: Uuid) -> Result<Vec<TaskNode>> {
        self.get_milestone_tasks(milestone_id).await
    }

    async fn get_release_tasks(&self, release_id: Uuid) -> Result<Vec<TaskNode>> {
        self.get_release_tasks(release_id).await
    }

    async fn get_project_progress(&self, project_id: Uuid) -> Result<(u32, u32, u32, u32)> {
        self.get_project_progress(project_id).await
    }

    async fn get_project_task_dependencies(&self, project_id: Uuid) -> Result<Vec<(Uuid, Uuid)>> {
        self.get_project_task_dependencies(project_id).await
    }

    async fn get_project_tasks(&self, project_id: Uuid) -> Result<Vec<TaskNode>> {
        self.get_project_tasks(project_id).await
    }

    async fn list_plans_filtered(
        &self,
        project_id: Option<Uuid>,
        workspace_slug: Option<&str>,
        statuses: Option<Vec<String>>,
        priority_min: Option<i32>,
        priority_max: Option<i32>,
        search: Option<&str>,
        limit: usize,
        offset: usize,
        sort_by: Option<&str>,
        sort_order: &str,
    ) -> Result<(Vec<PlanNode>, usize)> {
        self.list_plans_filtered(
            project_id,
            workspace_slug,
            statuses,
            priority_min,
            priority_max,
            search,
            limit,
            offset,
            sort_by,
            sort_order,
        )
        .await
    }

    async fn list_all_tasks_filtered(
        &self,
        plan_id: Option<Uuid>,
        project_id: Option<Uuid>,
        workspace_slug: Option<&str>,
        statuses: Option<Vec<String>>,
        priority_min: Option<i32>,
        priority_max: Option<i32>,
        tags: Option<Vec<String>>,
        assigned_to: Option<&str>,
        limit: usize,
        offset: usize,
        sort_by: Option<&str>,
        sort_order: &str,
    ) -> Result<(Vec<TaskWithPlan>, usize)> {
        self.list_all_tasks_filtered(
            plan_id,
            project_id,
            workspace_slug,
            statuses,
            priority_min,
            priority_max,
            tags,
            assigned_to,
            limit,
            offset,
            sort_by,
            sort_order,
        )
        .await
    }

    async fn list_releases_filtered(
        &self,
        project_id: Uuid,
        statuses: Option<Vec<String>>,
        limit: usize,
        offset: usize,
        sort_by: Option<&str>,
        sort_order: &str,
    ) -> Result<(Vec<ReleaseNode>, usize)> {
        self.list_releases_filtered(project_id, statuses, limit, offset, sort_by, sort_order)
            .await
    }

    async fn list_milestones_filtered(
        &self,
        project_id: Uuid,
        statuses: Option<Vec<String>>,
        limit: usize,
        offset: usize,
        sort_by: Option<&str>,
        sort_order: &str,
    ) -> Result<(Vec<MilestoneNode>, usize)> {
        self.list_milestones_filtered(project_id, statuses, limit, offset, sort_by, sort_order)
            .await
    }

    async fn list_projects_filtered(
        &self,
        search: Option<&str>,
        limit: usize,
        offset: usize,
        sort_by: Option<&str>,
        sort_order: &str,
    ) -> Result<(Vec<ProjectNode>, usize)> {
        self.list_projects_filtered(search, limit, offset, sort_by, sort_order)
            .await
    }

    async fn create_note(&self, note: &Note) -> Result<()> {
        self.create_note(note).await
    }

    async fn get_note(&self, id: Uuid) -> Result<Option<Note>> {
        self.get_note(id).await
    }

    async fn update_note(
        &self,
        id: Uuid,
        content: Option<String>,
        importance: Option<NoteImportance>,
        status: Option<NoteStatus>,
        tags: Option<Vec<String>>,
        staleness_score: Option<f64>,
    ) -> Result<Option<Note>> {
        self.update_note(id, content, importance, status, tags, staleness_score)
            .await
    }

    async fn delete_note(&self, id: Uuid) -> Result<bool> {
        self.delete_note(id).await
    }

    async fn list_notes(
        &self,
        project_id: Option<Uuid>,
        workspace_slug: Option<&str>,
        filters: &NoteFilters,
    ) -> Result<(Vec<Note>, usize)> {
        self.list_notes(project_id, workspace_slug, filters).await
    }

    async fn link_note_to_entity(
        &self,
        note_id: Uuid,
        entity_type: &EntityType,
        entity_id: &str,
        signature_hash: Option<&str>,
        body_hash: Option<&str>,
    ) -> Result<()> {
        self.link_note_to_entity(note_id, entity_type, entity_id, signature_hash, body_hash)
            .await
    }

    async fn unlink_note_from_entity(
        &self,
        note_id: Uuid,
        entity_type: &EntityType,
        entity_id: &str,
    ) -> Result<()> {
        self.unlink_note_from_entity(note_id, entity_type, entity_id)
            .await
    }

    async fn get_notes_for_entity(
        &self,
        entity_type: &EntityType,
        entity_id: &str,
    ) -> Result<Vec<Note>> {
        self.get_notes_for_entity(entity_type, entity_id).await
    }

    async fn get_propagated_notes(
        &self,
        entity_type: &EntityType,
        entity_id: &str,
        max_depth: u32,
        min_score: f64,
        relation_types: Option<&[String]>,
    ) -> Result<Vec<PropagatedNote>> {
        self.get_propagated_notes(entity_type, entity_id, max_depth, min_score, relation_types)
            .await
    }

    async fn get_workspace_notes_for_project(
        &self,
        project_id: Uuid,
        propagation_factor: f64,
    ) -> Result<Vec<PropagatedNote>> {
        self.get_workspace_notes_for_project(project_id, propagation_factor)
            .await
    }

    async fn supersede_note(&self, old_note_id: Uuid, new_note_id: Uuid) -> Result<()> {
        self.supersede_note(old_note_id, new_note_id).await
    }

    async fn confirm_note(&self, note_id: Uuid, confirmed_by: &str) -> Result<Option<Note>> {
        self.confirm_note(note_id, confirmed_by).await
    }

    async fn get_notes_needing_review(&self, project_id: Option<Uuid>) -> Result<Vec<Note>> {
        self.get_notes_needing_review(project_id).await
    }

    async fn update_staleness_scores(&self) -> Result<usize> {
        self.update_staleness_scores().await
    }

    async fn get_note_anchors(&self, note_id: Uuid) -> Result<Vec<NoteAnchor>> {
        self.get_note_anchors(note_id).await
    }

    async fn set_note_embedding(
        &self,
        note_id: Uuid,
        embedding: &[f32],
        model: &str,
    ) -> Result<()> {
        self.set_note_embedding(note_id, embedding, model).await
    }

    async fn get_note_embedding(&self, note_id: Uuid) -> Result<Option<Vec<f32>>> {
        self.get_note_embedding(note_id).await
    }

    async fn vector_search_notes(
        &self,
        embedding: &[f32],
        limit: usize,
        project_id: Option<Uuid>,
        workspace_slug: Option<&str>,
        min_similarity: Option<f64>,
    ) -> Result<Vec<(Note, f64)>> {
        self.vector_search_notes(embedding, limit, project_id, workspace_slug, min_similarity)
            .await
    }

    async fn list_notes_without_embedding(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<Note>, usize)> {
        self.list_notes_without_embedding(limit, offset).await
    }

    async fn set_file_embedding(
        &self,
        file_path: &str,
        embedding: &[f32],
        model: &str,
    ) -> Result<()> {
        self.set_file_embedding(file_path, embedding, model).await
    }

    async fn set_function_embedding(
        &self,
        function_name: &str,
        file_path: &str,
        embedding: &[f32],
        model: &str,
    ) -> Result<()> {
        self.set_function_embedding(function_name, file_path, embedding, model)
            .await
    }

    async fn vector_search_files(
        &self,
        embedding: &[f32],
        limit: usize,
        project_id: Option<Uuid>,
    ) -> Result<Vec<(String, f64)>> {
        self.vector_search_files(embedding, limit, project_id).await
    }

    async fn vector_search_functions(
        &self,
        embedding: &[f32],
        limit: usize,
        project_id: Option<Uuid>,
    ) -> Result<Vec<(String, String, f64)>> {
        self.vector_search_functions(embedding, limit, project_id)
            .await
    }

    async fn create_synapses(&self, note_id: Uuid, neighbors: &[(Uuid, f64)]) -> Result<usize> {
        self.create_synapses(note_id, neighbors).await
    }

    async fn get_synapses(&self, note_id: Uuid) -> Result<Vec<(Uuid, f64)>> {
        self.get_synapses(note_id).await
    }

    async fn delete_synapses(&self, note_id: Uuid) -> Result<usize> {
        self.delete_synapses(note_id).await
    }

    async fn update_energy_scores(&self, half_life_days: f64) -> Result<usize> {
        self.update_energy_scores(half_life_days).await
    }

    async fn boost_energy(&self, note_id: Uuid, amount: f64) -> Result<()> {
        self.boost_energy(note_id, amount).await
    }

    async fn reinforce_synapses(&self, note_ids: &[Uuid], boost: f64) -> Result<usize> {
        self.reinforce_synapses(note_ids, boost).await
    }

    async fn decay_synapses(
        &self,
        decay_amount: f64,
        prune_threshold: f64,
    ) -> Result<(usize, usize)> {
        self.decay_synapses(decay_amount, prune_threshold).await
    }

    async fn init_note_energy(&self) -> Result<usize> {
        self.init_note_energy().await
    }

    async fn list_notes_needing_synapses(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<cortex_core::notes::Note>, usize)> {
        self.list_notes_needing_synapses(limit, offset).await
    }

    async fn create_cross_entity_synapses(
        &self,
        source_id: Uuid,
        neighbors: &[(Uuid, f64)],
    ) -> Result<usize> {
        self.create_cross_entity_synapses(source_id, neighbors)
            .await
    }

    async fn get_cross_entity_synapses(&self, node_id: Uuid) -> Result<Vec<(Uuid, f64, String)>> {
        self.get_cross_entity_synapses(node_id).await
    }

    async fn list_decisions_needing_synapses(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<DecisionNode>, usize)> {
        self.list_decisions_needing_synapses(limit, offset).await
    }

    async fn create_chat_session(&self, session: &ChatSessionNode) -> Result<()> {
        self.create_chat_session(session).await
    }

    async fn get_chat_session(&self, id: Uuid) -> Result<Option<ChatSessionNode>> {
        self.get_chat_session(id).await
    }

    async fn list_chat_sessions(
        &self,
        project_slug: Option<&str>,
        workspace_slug: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<ChatSessionNode>, usize)> {
        self.list_chat_sessions(project_slug, workspace_slug, limit, offset)
            .await
    }

    async fn update_chat_session(
        &self,
        id: Uuid,
        cli_session_id: Option<String>,
        title: Option<String>,
        message_count: Option<i64>,
        total_cost_usd: Option<f64>,
        conversation_id: Option<String>,
        preview: Option<String>,
    ) -> Result<Option<ChatSessionNode>> {
        self.update_chat_session(
            id,
            cli_session_id,
            title,
            message_count,
            total_cost_usd,
            conversation_id,
            preview,
        )
        .await
    }

    async fn update_chat_session_permission_mode(&self, id: Uuid, mode: &str) -> Result<()> {
        self.update_chat_session_permission_mode(id, mode).await
    }

    async fn set_session_auto_continue(&self, id: Uuid, enabled: bool) -> Result<()> {
        self.set_session_auto_continue(id, enabled).await
    }

    async fn get_session_auto_continue(&self, id: Uuid) -> Result<bool> {
        self.get_session_auto_continue(id).await
    }

    async fn backfill_chat_session_previews(&self) -> Result<usize> {
        self.backfill_chat_session_previews().await
    }

    async fn delete_chat_session(&self, id: Uuid) -> Result<bool> {
        self.delete_chat_session(id).await
    }

    async fn store_chat_events(
        &self,
        session_id: Uuid,
        events: Vec<ChatEventRecord>,
    ) -> Result<()> {
        self.store_chat_events(session_id, events).await
    }

    async fn get_chat_events(
        &self,
        session_id: Uuid,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<ChatEventRecord>> {
        self.get_chat_events(session_id, after_seq, limit).await
    }

    async fn get_chat_events_paginated(
        &self,
        session_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<ChatEventRecord>> {
        self.get_chat_events_paginated(session_id, offset, limit)
            .await
    }

    async fn count_chat_events(&self, session_id: Uuid) -> Result<i64> {
        self.count_chat_events(session_id).await
    }

    async fn get_latest_chat_event_seq(&self, session_id: Uuid) -> Result<i64> {
        self.get_latest_chat_event_seq(session_id).await
    }

    async fn delete_chat_events(&self, session_id: Uuid) -> Result<()> {
        self.delete_chat_events(session_id).await
    }

    async fn add_discussed(
        &self,
        session_id: Uuid,
        entities: &[(String, String)],
    ) -> Result<usize> {
        self.add_discussed(session_id, entities).await
    }

    async fn get_session_entities(
        &self,
        session_id: Uuid,
        project_id: Option<Uuid>,
    ) -> Result<Vec<DiscussedEntity>> {
        self.get_session_entities(session_id, project_id).await
    }

    async fn backfill_discussed(&self) -> Result<(usize, usize, usize)> {
        self.backfill_discussed().await
    }

    async fn upsert_user(&self, user: &UserNode) -> Result<UserNode> {
        self.upsert_user(user).await
    }

    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<UserNode>> {
        self.get_user_by_id(id).await
    }

    async fn get_user_by_provider_id(
        &self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<UserNode>> {
        self.get_user_by_provider_id(provider, external_id).await
    }

    async fn get_user_by_email_and_provider(
        &self,
        email: &str,
        provider: &str,
    ) -> Result<Option<UserNode>> {
        self.get_user_by_email_and_provider(email, provider).await
    }

    async fn get_user_by_email(&self, email: &str) -> Result<Option<UserNode>> {
        self.get_user_by_email(email).await
    }

    async fn create_password_user(
        &self,
        email: &str,
        name: &str,
        password_hash: &str,
    ) -> Result<UserNode> {
        self.create_password_user(email, name, password_hash).await
    }

    async fn list_users(&self) -> Result<Vec<UserNode>> {
        self.list_users().await
    }

    async fn create_refresh_token(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        self.create_refresh_token(user_id, token_hash, expires_at)
            .await
    }

    async fn validate_refresh_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<cortex_core::models::RefreshTokenNode>> {
        self.validate_refresh_token(token_hash).await
    }

    async fn revoke_refresh_token(&self, token_hash: &str) -> Result<bool> {
        self.revoke_refresh_token(token_hash).await
    }

    async fn revoke_all_user_tokens(&self, user_id: Uuid) -> Result<u64> {
        self.revoke_all_user_tokens(user_id).await
    }

    async fn create_feature_graph(&self, graph: &FeatureGraphNode) -> Result<()> {
        self.create_feature_graph(graph).await
    }

    async fn get_feature_graph(&self, id: Uuid) -> Result<Option<FeatureGraphNode>> {
        self.get_feature_graph(id).await
    }

    async fn get_feature_graph_detail(&self, id: Uuid) -> Result<Option<FeatureGraphDetail>> {
        self.get_feature_graph_detail(id).await
    }

    async fn list_feature_graphs(&self, project_id: Option<Uuid>) -> Result<Vec<FeatureGraphNode>> {
        self.list_feature_graphs(project_id).await
    }

    async fn delete_feature_graph(&self, id: Uuid) -> Result<bool> {
        self.delete_feature_graph(id).await
    }

    async fn add_entity_to_feature_graph(
        &self,
        feature_graph_id: Uuid,
        entity_type: &str,
        entity_id: &str,
        role: Option<&str>,
        project_id: Option<Uuid>,
    ) -> Result<()> {
        self.add_entity_to_feature_graph(feature_graph_id, entity_type, entity_id, role, project_id)
            .await
    }

    async fn remove_entity_from_feature_graph(
        &self,
        feature_graph_id: Uuid,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<bool> {
        self.remove_entity_from_feature_graph(feature_graph_id, entity_type, entity_id)
            .await
    }

    async fn auto_build_feature_graph(
        &self,
        name: &str,
        description: Option<&str>,
        project_id: Uuid,
        entry_function: &str,
        depth: u32,
        include_relations: Option<&[String]>,
        filter_community: Option<bool>,
    ) -> Result<FeatureGraphDetail> {
        self.auto_build_feature_graph(
            name,
            description,
            project_id,
            entry_function,
            depth,
            include_relations,
            filter_community,
        )
        .await
    }

    async fn refresh_feature_graph(&self, id: Uuid) -> Result<Option<FeatureGraphDetail>> {
        self.refresh_feature_graph(id).await
    }

    async fn get_top_entry_functions(&self, project_id: Uuid, limit: usize) -> Result<Vec<String>> {
        self.get_top_entry_functions(project_id, limit).await
    }

    async fn get_project_import_edges(&self, project_id: Uuid) -> Result<Vec<(String, String)>> {
        self.get_project_import_edges(project_id).await
    }

    async fn get_project_call_edges(&self, project_id: Uuid) -> Result<Vec<(String, String)>> {
        self.get_project_call_edges(project_id).await
    }

    async fn get_project_extends_edges(&self, project_id: Uuid) -> Result<Vec<(String, String)>> {
        self.get_project_extends_edges(project_id).await
    }

    async fn get_project_implements_edges(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<(String, String)>> {
        self.get_project_implements_edges(project_id).await
    }

    async fn batch_update_file_analytics(&self, updates: &[FileAnalyticsUpdate]) -> Result<()> {
        self.batch_update_file_analytics(updates).await
    }

    async fn batch_update_function_analytics(
        &self,
        updates: &[FunctionAnalyticsUpdate],
    ) -> Result<()> {
        self.batch_update_function_analytics(updates).await
    }

    async fn batch_update_fabric_file_analytics(
        &self,
        updates: &[FabricFileAnalyticsUpdate],
    ) -> Result<()> {
        self.batch_update_fabric_file_analytics(updates).await
    }

    async fn batch_update_structural_dna(
        &self,
        updates: &[cortex_core::graph::StructuralDnaUpdate],
    ) -> Result<()> {
        self.batch_update_structural_dna(updates).await
    }

    async fn write_predicted_links(
        &self,
        project_id: &str,
        links: &[cortex_core::graph::LinkPrediction],
    ) -> Result<()> {
        self.write_predicted_links(project_id, links).await
    }

    async fn get_project_structural_dna(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, Vec<f64>)>> {
        self.get_project_structural_dna(project_id).await
    }

    async fn batch_update_structural_fingerprints(
        &self,
        updates: &[cortex_core::graph::StructuralFingerprintUpdate],
    ) -> Result<()> {
        self.batch_update_structural_fingerprints(updates).await
    }

    async fn get_project_structural_fingerprints(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, Vec<f64>)>> {
        self.get_project_structural_fingerprints(project_id).await
    }

    async fn get_project_file_signals(
        &self,
        project_id: &str,
    ) -> Result<Vec<cortex_core::graph::FileSignalRecord>> {
        self.get_project_file_signals(project_id).await
    }

    async fn get_project_synapse_edges(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<(String, String, f64)>> {
        self.get_project_synapse_edges(project_id).await
    }

    async fn get_neural_metrics(
        &self,
        project_id: Uuid,
    ) -> Result<cortex_core::models::NeuralMetrics> {
        self.get_neural_metrics(project_id).await
    }

    async fn compute_churn_scores(&self, project_id: Uuid) -> Result<Vec<FileChurnScore>> {
        self.compute_churn_scores(project_id).await
    }

    async fn batch_update_churn_scores(&self, updates: &[FileChurnScore]) -> Result<()> {
        self.batch_update_churn_scores(updates).await
    }

    async fn get_top_hotspots(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> Result<Vec<FileChurnScore>> {
        self.get_top_hotspots(project_id, limit).await
    }

    async fn compute_knowledge_density(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<FileKnowledgeDensity>> {
        self.compute_knowledge_density(project_id).await
    }

    async fn batch_update_knowledge_density(&self, updates: &[FileKnowledgeDensity]) -> Result<()> {
        self.batch_update_knowledge_density(updates).await
    }

    async fn get_top_knowledge_gaps(
        &self,
        project_id: Uuid,
        limit: usize,
    ) -> Result<Vec<FileKnowledgeDensity>> {
        self.get_top_knowledge_gaps(project_id, limit).await
    }

    async fn compute_risk_scores(&self, project_id: Uuid) -> Result<Vec<FileRiskScore>> {
        self.compute_risk_scores(project_id).await
    }

    async fn batch_update_risk_scores(&self, updates: &[FileRiskScore]) -> Result<()> {
        self.batch_update_risk_scores(updates).await
    }

    async fn get_risk_summary(&self, project_id: Uuid) -> Result<serde_json::Value> {
        self.get_risk_summary(project_id).await
    }

    async fn batch_upsert_processes(&self, processes: &[ProcessNode]) -> Result<()> {
        self.batch_upsert_processes(processes).await
    }

    async fn batch_create_step_relationships(&self, steps: &[(String, String, u32)]) -> Result<()> {
        self.batch_create_step_relationships(steps).await
    }

    async fn delete_project_processes(&self, project_id: Uuid) -> Result<u64> {
        self.delete_project_processes(project_id).await
    }

    async fn create_skill(&self, skill: &SkillNode) -> Result<()> {
        self.create_skill(skill).await
    }

    async fn get_skill(&self, id: Uuid) -> Result<Option<SkillNode>> {
        self.get_skill(id).await
    }

    async fn update_skill(&self, skill: &SkillNode) -> Result<()> {
        self.update_skill(skill).await
    }

    async fn delete_skill(&self, id: Uuid) -> Result<bool> {
        self.delete_skill(id).await
    }

    async fn list_skills(
        &self,
        project_id: Uuid,
        status: Option<SkillStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<SkillNode>, usize)> {
        self.list_skills(project_id, status, limit, offset).await
    }

    async fn get_skill_members(&self, skill_id: Uuid) -> Result<(Vec<Note>, Vec<DecisionNode>)> {
        self.get_skill_members(skill_id).await
    }

    async fn add_skill_member(
        &self,
        skill_id: Uuid,
        entity_type: &str,
        entity_id: Uuid,
    ) -> Result<()> {
        self.add_skill_member(skill_id, entity_type, entity_id)
            .await
    }

    async fn remove_skill_member(
        &self,
        skill_id: Uuid,
        entity_type: &str,
        entity_id: Uuid,
    ) -> Result<bool> {
        self.remove_skill_member(skill_id, entity_type, entity_id)
            .await
    }

    async fn remove_all_skill_members(&self, skill_id: Uuid) -> Result<i64> {
        self.remove_all_skill_members(skill_id).await
    }

    async fn get_skills_for_note(&self, note_id: Uuid) -> Result<Vec<SkillNode>> {
        self.get_skills_for_note(note_id).await
    }

    async fn get_skills_for_project(&self, project_id: Uuid) -> Result<Vec<SkillNode>> {
        self.get_skills_for_project(project_id).await
    }

    async fn activate_skill(&self, skill_id: Uuid, query: &str) -> Result<ActivatedSkillContext> {
        self.activate_skill(skill_id, query).await
    }

    async fn increment_skill_activation(&self, skill_id: Uuid) -> Result<()> {
        self.increment_skill_activation(skill_id).await
    }

    async fn match_skills_by_trigger(
        &self,
        project_id: Uuid,
        input: &str,
    ) -> Result<Vec<(SkillNode, f64)>> {
        self.match_skills_by_trigger(project_id, input).await
    }

    async fn get_synapse_graph(
        &self,
        project_id: Uuid,
        min_weight: f64,
    ) -> Result<Vec<(String, String, f64)>> {
        self.get_synapse_graph(project_id, min_weight).await
    }

    async fn create_analysis_profile(&self, profile: &AnalysisProfile) -> Result<()> {
        self.create_analysis_profile(profile).await
    }

    async fn list_analysis_profiles(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<AnalysisProfile>> {
        self.list_analysis_profiles(project_id).await
    }

    async fn get_analysis_profile(&self, id: &str) -> Result<Option<AnalysisProfile>> {
        self.get_analysis_profile(id).await
    }

    async fn delete_analysis_profile(&self, id: &str) -> Result<()> {
        self.delete_analysis_profile(id).await
    }

    async fn find_bridge_subgraph(
        &self,
        source: &str,
        target: &str,
        _max_hops: u32,
        _relation_types: &[String],
        project_id: &str,
    ) -> Result<(
        Vec<cortex_core::graph::BridgeRawNode>,
        Vec<cortex_core::graph::BridgeRawEdge>,
    )> {
        let pid = Uuid::parse_str(project_id).unwrap_or_default();
        self.find_bridge_subgraph(source, target, pid).await
    }

    async fn get_knowledge_density(&self, file_path: &str, project_id: &str) -> Result<f64> {
        self.get_knowledge_density(file_path, project_id).await
    }

    async fn get_node_pagerank(&self, file_path: &str, project_id: &str) -> Result<f64> {
        self.get_node_pagerank(file_path, project_id).await
    }

    async fn get_bridge_proximity(
        &self,
        file_path: &str,
        project_id: &str,
    ) -> Result<Vec<(String, f64)>> {
        self.get_bridge_proximity(file_path, project_id).await
    }

    async fn get_avg_multi_signal_score(&self, project_id: Uuid) -> Result<f64> {
        self.get_avg_multi_signal_score(project_id).await
    }

    async fn create_topology_rule(&self, rule: &TopologyRule) -> Result<()> {
        self.create_topology_rule(rule).await
    }

    async fn list_topology_rules(&self, project_id: &str) -> Result<Vec<TopologyRule>> {
        self.list_topology_rules(project_id).await
    }

    async fn delete_topology_rule(&self, rule_id: &str) -> Result<()> {
        self.delete_topology_rule(rule_id).await
    }

    async fn check_topology_rules(&self, project_id: &str) -> Result<Vec<TopologyViolation>> {
        let pid = Uuid::parse_str(project_id).unwrap_or_default();
        self.check_topology_rules_code(pid).await
    }

    async fn check_file_topology(
        &self,
        project_id: &str,
        file_path: &str,
        new_imports: &[String],
    ) -> Result<Vec<TopologyViolation>> {
        let pid = Uuid::parse_str(project_id).unwrap_or_default();
        self.check_file_topology_code(pid, file_path, new_imports)
            .await
    }

    async fn health_check(&self) -> Result<bool> {
        self.health_check().await
    }

    async fn batch_save_context_cards(
        &self,
        cards: &[cortex_core::graph::ContextCard],
    ) -> Result<()> {
        self.batch_save_context_cards(cards).await
    }

    async fn invalidate_context_cards(&self, paths: &[String], project_id: &str) -> Result<()> {
        self.invalidate_context_cards(paths, project_id).await
    }

    async fn get_context_card(
        &self,
        path: &str,
        project_id: &str,
    ) -> Result<Option<cortex_core::graph::ContextCard>> {
        self.get_context_card(path, project_id).await
    }

    async fn get_context_cards_batch(
        &self,
        paths: &[String],
        project_id: &str,
    ) -> Result<Vec<cortex_core::graph::ContextCard>> {
        self.get_context_cards_batch(paths, project_id).await
    }

    async fn find_isomorphic_groups(
        &self,
        project_id: &str,
        min_group_size: usize,
    ) -> Result<Vec<cortex_core::graph::IsomorphicGroup>> {
        let pid = Uuid::parse_str(project_id).unwrap_or_default();
        self.find_isomorphic_groups(pid, min_group_size as f64)
            .await
    }

    async fn has_context_cards(&self, project_id: &str) -> Result<bool> {
        self.has_context_cards(project_id).await
    }

    async fn search_notes_fts(
        &self,
        query: &str,
        limit: usize,
        project_id: Option<&str>,
    ) -> Result<Vec<(cortex_core::notes::Note, f64)>> {
        self.search_notes_fts(query, limit, project_id).await
    }

    async fn search_decisions_fts(
        &self,
        query: &str,
        limit: usize,
        project_id: Option<&str>,
    ) -> Result<Vec<(cortex_core::models::DecisionNode, f64)>> {
        self.search_decisions_fts(query, limit, project_id).await
    }

    async fn search_code_fts(
        &self,
        query: &str,
        limit: usize,
        project_id: Option<&str>,
        language: Option<&str>,
    ) -> Result<Vec<cortex_graph::CodeSearchHit>> {
        self.search_code_fts(query, limit, project_id, language)
            .await
    }

    async fn add_episode(
        &self,
        req: cortex_core::episode::CreateEpisodeRequest,
    ) -> Result<cortex_core::episode::Episode> {
        self.add_episode(req).await
    }

    async fn get_episodes(
        &self,
        project_id: Option<&str>,
        group_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<cortex_core::episode::Episode>> {
        self.get_episodes(project_id, group_id, limit).await
    }

    async fn search_episodes(
        &self,
        query: &str,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<cortex_core::episode::Episode>> {
        self.search_episodes(query, project_id, limit).await
    }

    async fn invalidate_note_at(&self, id: &str, at: DateTime<Utc>) -> Result<()> {
        self.invalidate_note_at(id, at).await
    }

    async fn search_notes_at_time(
        &self,
        query: &str,
        at: DateTime<Utc>,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<cortex_core::notes::Note>> {
        self.search_notes_at_time(query, at, project_id, limit)
            .await
    }
}
