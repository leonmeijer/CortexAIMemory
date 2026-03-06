//! Test factory functions for domain types.
//!
//! Provides convenient constructors for use in tests across all workspace crates.

use crate::models::*;
use chrono::Utc;
use uuid::Uuid;

/// Create a test project with sensible defaults
pub fn test_project() -> ProjectNode {
    ProjectNode {
        id: Uuid::new_v4(),
        name: "test-project".to_string(),
        slug: "test-project".to_string(),
        root_path: "/tmp/test-project".to_string(),
        description: Some("A test project".to_string()),
        created_at: Utc::now(),
        last_synced: None,
        analytics_computed_at: None,
        last_co_change_computed_at: None,
    }
}

/// Create a test project with a specific name
pub fn test_project_named(name: &str) -> ProjectNode {
    ProjectNode {
        id: Uuid::new_v4(),
        name: name.to_string(),
        slug: name.to_lowercase().replace(' ', "-"),
        root_path: format!("/tmp/{}", name.to_lowercase().replace(' ', "-")),
        description: Some(format!("Test project: {}", name)),
        created_at: Utc::now(),
        last_synced: None,
        analytics_computed_at: None,
        last_co_change_computed_at: None,
    }
}

/// Create a test file with sensible defaults
pub fn test_file(path: &str) -> FileNode {
    FileNode {
        path: path.to_string(),
        language: "rust".to_string(),
        hash: format!("hash_{}", path.replace('/', "_")),
        last_parsed: Utc::now(),
        project_id: None,
    }
}

/// Create a test file linked to a project
pub fn test_file_for_project(path: &str, project_id: Uuid) -> FileNode {
    FileNode {
        path: path.to_string(),
        language: "rust".to_string(),
        hash: format!("hash_{}", path.replace('/', "_")),
        last_parsed: Utc::now(),
        project_id: Some(project_id),
    }
}

/// Create a test function node
pub fn test_function(name: &str, file_path: &str) -> FunctionNode {
    FunctionNode {
        name: name.to_string(),
        visibility: Visibility::Public,
        params: vec![],
        return_type: None,
        generics: vec![],
        is_async: false,
        is_unsafe: false,
        complexity: 1,
        file_path: file_path.to_string(),
        line_start: 1,
        line_end: 10,
        docstring: None,
    }
}

/// Create a test struct node
pub fn test_struct(name: &str, file_path: &str) -> StructNode {
    StructNode {
        name: name.to_string(),
        visibility: Visibility::Public,
        generics: vec![],
        file_path: file_path.to_string(),
        line_start: 1,
        line_end: 10,
        docstring: None,
        parent_class: None,
        interfaces: vec![],
    }
}

/// Create a test trait node
pub fn test_trait(name: &str, file_path: &str) -> TraitNode {
    TraitNode {
        name: name.to_string(),
        visibility: Visibility::Public,
        generics: vec![],
        file_path: file_path.to_string(),
        line_start: 1,
        line_end: 10,
        docstring: None,
        is_external: false,
        source: None,
    }
}

/// Create a test enum node
pub fn test_enum(name: &str, file_path: &str) -> EnumNode {
    EnumNode {
        name: name.to_string(),
        visibility: Visibility::Public,
        variants: vec!["A".to_string(), "B".to_string()],
        file_path: file_path.to_string(),
        line_start: 1,
        line_end: 10,
        docstring: None,
    }
}

/// Create a test impl node
pub fn test_impl(for_type: &str, file_path: &str) -> ImplNode {
    ImplNode {
        for_type: for_type.to_string(),
        trait_name: None,
        generics: vec![],
        where_clause: None,
        file_path: file_path.to_string(),
        line_start: 1,
        line_end: 10,
    }
}

/// Create a test import node
pub fn test_import(path: &str, file_path: &str) -> ImportNode {
    ImportNode {
        path: path.to_string(),
        alias: None,
        items: vec![],
        file_path: file_path.to_string(),
        line: 1,
    }
}

/// Create a test plan for a specific project
pub fn test_plan_for_project(project_id: Uuid) -> PlanNode {
    PlanNode::new_for_project(
        "Test Plan".to_string(),
        "A test plan for unit testing".to_string(),
        "test-agent".to_string(),
        5,
        project_id,
    )
}

/// Create a test plan (no project)
pub fn test_plan() -> PlanNode {
    PlanNode::new(
        "Test Plan".to_string(),
        "A test plan for unit testing".to_string(),
        "test-agent".to_string(),
        5,
    )
}

/// Create a test task
pub fn test_task(description: &str) -> TaskNode {
    TaskNode::new(description.to_string())
}

/// Create a test step
pub fn test_step(order: u32, description: &str) -> StepNode {
    StepNode::new(order, description.to_string(), None)
}

/// Create a test decision
pub fn test_decision() -> DecisionNode {
    DecisionNode::new(
        "Use SurrealDB".to_string(),
        "Better Rust integration".to_string(),
        vec!["PostgreSQL".to_string(), "MongoDB".to_string()],
        "test-architect".to_string(),
    )
}

/// Create a test constraint
pub fn test_constraint() -> ConstraintNode {
    ConstraintNode::new(
        ConstraintType::Performance,
        "Response time under 100ms".to_string(),
        None,
    )
}

/// Create a test commit
pub fn test_commit(hash: &str) -> CommitNode {
    CommitNode {
        hash: hash.to_string(),
        message: format!("test commit {}", hash),
        author: "test-author".to_string(),
        timestamp: Utc::now(),
    }
}

/// Create a test release
pub fn test_release(project_id: Uuid) -> ReleaseNode {
    ReleaseNode {
        id: Uuid::new_v4(),
        version: "1.0.0".to_string(),
        title: Some("Initial Release".to_string()),
        description: Some("First release".to_string()),
        status: ReleaseStatus::Planned,
        target_date: None,
        released_at: None,
        created_at: Utc::now(),
        project_id,
    }
}

/// Create a test milestone
pub fn test_milestone(project_id: Uuid) -> MilestoneNode {
    MilestoneNode {
        id: Uuid::new_v4(),
        title: "v1.0 Milestone".to_string(),
        description: Some("First milestone".to_string()),
        status: MilestoneStatus::Open,
        target_date: None,
        closed_at: None,
        created_at: Utc::now(),
        project_id,
    }
}
