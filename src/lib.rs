//! A standard `EXPLAIN ANALYZE`-style plan for Snowflake queries.
//!
//! Snowflake has no `EXPLAIN ANALYZE`. The closest data is
//! `GET_QUERY_OPERATOR_STATS`, which returns one flat row per operator.
//! This crate turns those rows into a [`QueryPlan`]: a small, typed model that
//! different tools can share, either as Rust values or as JSON.
//!
//! ```
//! use snowflake_explain::{QueryPlan, rows_from_json};
//!
//! // The output of: SELECT * FROM TABLE(GET_QUERY_OPERATOR_STATS('<query id>'))
//! let rows = rows_from_json(r#"[
//!     {"QUERY_ID": "q1", "STEP_ID": 1, "OPERATOR_ID": 0, "PARENT_OPERATORS": null,
//!      "OPERATOR_TYPE": "Result",
//!      "OPERATOR_STATISTICS": {"input_rows": 5, "output_rows": 5},
//!      "EXECUTION_TIME_BREAKDOWN": {"overall_percentage": 0.25}},
//!     {"QUERY_ID": "q1", "STEP_ID": 1, "OPERATOR_ID": 1, "PARENT_OPERATORS": [0],
//!      "OPERATOR_TYPE": "TableScan",
//!      "OPERATOR_STATISTICS": {"output_rows": 5},
//!      "EXECUTION_TIME_BREAKDOWN": {"overall_percentage": 0.75},
//!      "OPERATOR_ATTRIBUTES": {"table_name": "DB.PUBLIC.T"}}
//! ]"#).unwrap();
//!
//! // Optionally attribute the query's execution time (from QUERY_HISTORY) to operators.
//! let plan = QueryPlan::from_rows(rows).unwrap().with_execution_time(200);
//!
//! assert_eq!(plan.to_string(), "\
//! Step 1
//! -> Result [0]  rows=5  time=25.0%  ~50ms
//!    -> TableScan [1]  rows=5  time=75.0%  ~150ms
//! ");
//! ```

mod build;
mod error;
mod plan;
mod render;
mod row;

pub use error::BuildError;
pub use plan::{Node, QueryPlan, Stats, Step, TimeBreakdown};
pub use row::{OperatorStatsRow, rows_from_json};
