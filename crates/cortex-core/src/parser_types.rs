//! Parser-related types that are used in the GraphStore trait interface.

/// Represents a function call found in code
#[derive(Debug, Clone)]
pub struct FunctionCall {
    /// The function making the call
    pub caller_id: String,
    /// The name of the function being called
    pub callee_name: String,
    /// Line where the call occurs
    pub line: u32,
    /// Confidence score (0.0-1.0) for the call relationship.
    pub confidence: f64,
    /// Reason for the confidence level
    pub reason: String,
}
