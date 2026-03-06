//! Integration tests for project-orchestrator
//!
//! These tests require IndentiaGraph to be running.
//! Run with: cargo test --test integration_tests

use project_orchestrator::indentiagraph::models::*;
use project_orchestrator::{AppState, Config};
use uuid::Uuid;

/// Get test configuration from environment or use defaults
fn test_config() -> Config {
    Config {
        setup_completed: true,
        indentiagraph_uri: std::env::var("INDENTIAGRAPH_URI")
            .unwrap_or_else(|_| "ws://localhost:8000".into()),
        indentiagraph_user: std::env::var("INDENTIAGRAPH_USER").unwrap_or_else(|_| "root".into()),
        indentiagraph_password: std::env::var("INDENTIAGRAPH_PASSWORD")
            .unwrap_or_else(|_| "root".into()),
        surreal_namespace: std::env::var("SURREALDB_NAMESPACE").unwrap_or_else(|_| "cortex".into()),
        surreal_database: std::env::var("SURREALDB_DATABASE").unwrap_or_else(|_| "memory".into()),
        nats_url: None,
        workspace_path: ".".into(),
        server_port: 8080,
        auth_config: None,
        serve_frontend: false,
        frontend_path: "./dist".to_string(),
        public_url: None,
        chat_permissions: None,
        chat_default_model: None,
        chat_max_sessions: None,
        chat_max_turns: None,
        chat_session_timeout_secs: None,
        chat_process_path: None,
        chat_claude_cli_path: None,
        chat_auto_update_cli: None,
        chat_auto_update_app: None,
        embedding_provider: None,
        embedding_fastembed_model: None,
        embedding_fastembed_cache_dir: None,
        embedding_url: None,
        embedding_model: None,
        embedding_api_key: None,
        embedding_dimensions: None,
        config_yaml_path: None,
    }
}

/// Check if backends are available
async fn backends_available() -> bool {
    AppState::new(test_config()).await.is_ok()
}

#[tokio::test]
async fn test_app_state_initialization() {
    if !backends_available().await {
        eprintln!("Skipping test: backends not available");
        return;
    }

    let config = test_config();
    let state = AppState::new(config).await;

    assert!(state.is_ok(), "AppState should initialize successfully");
}

#[tokio::test]
async fn test_indentiagraph_file_operations() {
    if !backends_available().await {
        eprintln!("Skipping test: backends not available");
        return;
    }

    let config = test_config();
    let state = AppState::new(config).await.unwrap();

    // Create a test file node
    let file = FileNode {
        path: format!("/test/file_{}.rs", Uuid::new_v4()),
        language: "rust".to_string(),
        hash: "abc123".to_string(),
        last_parsed: chrono::Utc::now(),
        project_id: None,
    };

    // Upsert file
    let result = state.indentiagraph.upsert_file(&file).await;
    assert!(result.is_ok(), "Should upsert file: {:?}", result.err());

    // Get file
    let retrieved = state.indentiagraph.get_file(&file.path).await.unwrap();
    assert!(retrieved.is_some(), "Should retrieve file");

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.path, file.path);
    assert_eq!(retrieved.language, file.language);
    assert_eq!(retrieved.hash, file.hash);

    // Cleanup: delete the test file
    state.indentiagraph.delete_file(&file.path).await.unwrap();
}

#[tokio::test]
async fn test_indentiagraph_plan_operations() {
    if !backends_available().await {
        eprintln!("Skipping test: backends not available");
        return;
    }

    let config = test_config();
    let state = AppState::new(config).await.unwrap();

    // Create a test plan
    let plan = PlanNode::new(
        format!("Test Plan {}", Uuid::new_v4()),
        "Test description".to_string(),
        "test-agent".to_string(),
        5,
    );

    // Create plan
    let result = state.indentiagraph.create_plan(&plan).await;
    assert!(result.is_ok(), "Should create plan: {:?}", result.err());

    // Get plan
    let retrieved = state.indentiagraph.get_plan(plan.id).await.unwrap();
    assert!(retrieved.is_some(), "Should retrieve plan");

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, plan.id);
    assert_eq!(retrieved.title, plan.title);

    // Update status
    let result = state
        .indentiagraph
        .update_plan_status(plan.id, PlanStatus::Approved)
        .await;
    assert!(result.is_ok(), "Should update plan status");

    // Verify status update
    let updated = state
        .indentiagraph
        .get_plan(plan.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, PlanStatus::Approved);

    // Cleanup: delete the test plan
    state.indentiagraph.delete_plan(plan.id).await.unwrap();
}

#[tokio::test]
async fn test_indentiagraph_task_operations() {
    if !backends_available().await {
        eprintln!("Skipping test: backends not available");
        return;
    }

    let config = test_config();
    let state = AppState::new(config).await.unwrap();

    // Create a plan first
    let plan = PlanNode::new(
        format!("Task Test Plan {}", Uuid::new_v4()),
        "Plan for task testing".to_string(),
        "test-agent".to_string(),
        1,
    );
    state.indentiagraph.create_plan(&plan).await.unwrap();

    // Create a task
    let task = TaskNode::new("Test task description".to_string());
    let result = state.indentiagraph.create_task(plan.id, &task).await;
    assert!(result.is_ok(), "Should create task: {:?}", result.err());

    // Get tasks for plan
    let tasks = state.indentiagraph.get_plan_tasks(plan.id).await.unwrap();
    assert_eq!(tasks.len(), 1, "Should have one task");
    assert_eq!(tasks[0].id, task.id);

    // Update task status
    let result = state
        .indentiagraph
        .update_task_status(task.id, TaskStatus::InProgress)
        .await;
    assert!(result.is_ok(), "Should update task status");

    // Get next available task (should be none since our task is in progress)
    let next = state
        .indentiagraph
        .get_next_available_task(plan.id)
        .await
        .unwrap();
    assert!(next.is_none(), "No pending tasks should be available");

    // Cleanup: delete the test plan (which will also delete tasks)
    state.indentiagraph.delete_plan(plan.id).await.unwrap();
}

#[tokio::test]
async fn test_indentiagraph_task_dependencies() {
    if !backends_available().await {
        eprintln!("Skipping test: backends not available");
        return;
    }

    let config = test_config();
    let state = AppState::new(config).await.unwrap();

    // Create a plan
    let plan = PlanNode::new(
        format!("Dependency Test Plan {}", Uuid::new_v4()),
        "Plan for dependency testing".to_string(),
        "test-agent".to_string(),
        1,
    );
    state.indentiagraph.create_plan(&plan).await.unwrap();

    // Create task 1 (no dependencies)
    let task1 = TaskNode::new("Task 1 - Foundation".to_string());
    state
        .indentiagraph
        .create_task(plan.id, &task1)
        .await
        .unwrap();

    // Create task 2 (depends on task 1)
    let task2 = TaskNode::new("Task 2 - Depends on Task 1".to_string());
    state
        .indentiagraph
        .create_task(plan.id, &task2)
        .await
        .unwrap();
    state
        .indentiagraph
        .add_task_dependency(task2.id, task1.id)
        .await
        .unwrap();

    // Get next available task - should be task1 (task2 is blocked)
    let next = state
        .indentiagraph
        .get_next_available_task(plan.id)
        .await
        .unwrap();
    assert!(next.is_some(), "Should have an available task");
    assert_eq!(next.unwrap().id, task1.id, "Task 1 should be available");

    // Complete task 1
    state
        .indentiagraph
        .update_task_status(task1.id, TaskStatus::Completed)
        .await
        .unwrap();

    // Now task 2 should be available
    let next = state
        .indentiagraph
        .get_next_available_task(plan.id)
        .await
        .unwrap();
    assert!(next.is_some(), "Task 2 should now be available");
    assert_eq!(next.unwrap().id, task2.id);

    // Cleanup: delete the test plan (which will also delete tasks)
    state.indentiagraph.delete_plan(plan.id).await.unwrap();
}

#[tokio::test]
async fn test_indentiagraph_stale_file_cleanup() {
    if !backends_available().await {
        eprintln!("Skipping test: backends not available");
        return;
    }

    let config = test_config();
    let state = AppState::new(config).await.unwrap();

    // Create a test project
    let project_id = Uuid::new_v4();
    let project = project_orchestrator::indentiagraph::models::ProjectNode {
        id: project_id,
        name: format!("Cleanup Test Project {}", project_id),
        slug: format!("cleanup-test-{}", project_id),
        root_path: "/tmp/test-cleanup".to_string(),
        description: Some("Project for testing stale file cleanup".to_string()),
        created_at: chrono::Utc::now(),
        last_synced: None,
        analytics_computed_at: None,
        last_co_change_computed_at: None,
    };
    state.indentiagraph.create_project(&project).await.unwrap();

    // Create some file nodes belonging to this project
    let file1_path = format!("/tmp/test-cleanup/file1_{}.rs", Uuid::new_v4());
    let file2_path = format!("/tmp/test-cleanup/file2_{}.rs", Uuid::new_v4());
    let file3_path = format!("/tmp/test-cleanup/file3_{}.rs", Uuid::new_v4());

    for path in [&file1_path, &file2_path, &file3_path] {
        let file = FileNode {
            path: path.clone(),
            language: "rust".to_string(),
            hash: "test-hash".to_string(),
            last_parsed: chrono::Utc::now(),
            project_id: Some(project_id),
        };
        state.indentiagraph.upsert_file(&file).await.unwrap();
        state
            .indentiagraph
            .link_file_to_project(path, project_id)
            .await
            .unwrap();
    }

    // Verify all 3 files exist
    let paths_before = state
        .indentiagraph
        .get_project_file_paths(project_id)
        .await
        .unwrap();
    assert_eq!(paths_before.len(), 3, "Should have 3 files before cleanup");

    // Now simulate a sync where only file1 and file2 exist (file3 was deleted)
    let valid_paths = vec![file1_path.clone(), file2_path.clone()];
    let (files_deleted, _symbols_deleted, deleted_paths) = state
        .indentiagraph
        .delete_stale_files(project_id, &valid_paths)
        .await
        .unwrap();

    assert_eq!(files_deleted, 1, "Should delete 1 stale file");
    assert_eq!(deleted_paths.len(), 1, "Should return 1 deleted path");

    // Verify only 2 files remain
    let paths_after = state
        .indentiagraph
        .get_project_file_paths(project_id)
        .await
        .unwrap();
    assert_eq!(paths_after.len(), 2, "Should have 2 files after cleanup");
    assert!(
        paths_after.contains(&file1_path),
        "file1 should still exist"
    );
    assert!(
        paths_after.contains(&file2_path),
        "file2 should still exist"
    );
    assert!(
        !paths_after.contains(&file3_path),
        "file3 should be deleted"
    );

    // Cleanup: delete the test project
    state
        .indentiagraph
        .delete_project(project_id, "test-project")
        .await
        .unwrap();
}
