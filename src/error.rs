use thiserror::Error;

/// Why a set of operator rows could not be turned into a [`QueryPlan`](crate::QueryPlan).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuildError {
    #[error("no operator rows were given")]
    Empty,

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
