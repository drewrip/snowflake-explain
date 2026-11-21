use thiserror::Error;

/// Why operator rows or an `EXPLAIN` document could not be turned into a [`QueryPlan`](crate::QueryPlan).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuildError {
    #[error("no operators were given")]
    Empty,

    #[error("not an `EXPLAIN USING JSON` plan: {0}")]
    InvalidExplain(String),

    #[error("rows belong to more than one query: {0} and {1}")]
    MixedQueryIds(String, String),

    #[error("step {step} contains operator {operator} more than once")]
    DuplicateOperator { step: u32, operator: u32 },

    #[error("operator {operator} in step {step} names unknown parent {parent}")]
    UnknownParent {
        step: u32,
        operator: u32,
        parent: u32,
    },
}
