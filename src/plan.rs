use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The executed plan of one Snowflake query: Snowflake's answer to `EXPLAIN ANALYZE`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryPlan {
    pub query_id: String,
    /// Ordered by step id.
    pub steps: Vec<Step>,
}

/// One step of a query. Each step has its own graph of operators.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub id: u32,
    /// Ordered by operator id.
    pub nodes: Vec<Node>,
}

/// One operator in a step.
///
/// Operators form a DAG, not a tree: a shared operator (such as the
/// `WithClause` behind a CTE) can have several parents. Nodes refer to each
/// other by operator id, so a shared node is stored exactly once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Operator id, unique within the step.
    pub id: u32,
    /// Operator type exactly as Snowflake names it, e.g. `TableScan` or `Join`.
    pub operator: String,
    /// Operators that consume this one's output. Empty for the root.
    pub parents: Vec<u32>,
    /// Operators that feed this one: the inverse of `parents`.
    pub children: Vec<u32>,
    pub stats: Stats,
    pub time: TimeBreakdown,
    /// Type-specific details such as `table_name` or `join_type`, kept as Snowflake gives them.
    pub attributes: Map<String, Value>,
    /// Estimated milliseconds attributed to this operator. Set only by
    /// [`QueryPlan::with_execution_time`].
    pub estimated_ms: Option<f64>,
}

/// The commonly needed operator statistics, plus everything else Snowflake reported.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub input_rows: Option<u64>,
    pub output_rows: Option<u64>,
    pub bytes_scanned: Option<u64>,
    pub partitions_scanned: Option<u64>,
    pub partitions_total: Option<u64>,
    pub bytes_spilled_local: Option<u64>,
    pub bytes_spilled_remote: Option<u64>,
    pub network_bytes: Option<u64>,
    /// The remaining statistics, in Snowflake's original nested layout.
    pub other: Map<String, Value>,
}

/// Each operator's share of the query's total execution time, as reported by
/// Snowflake (a fraction, where 1.0 is the whole query).
///
/// `overall` is the operator's total share; the other fields split it by activity.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimeBreakdown {
    #[serde(alias = "overall_percentage")]
    pub overall: Option<f64>,
    pub initialization: Option<f64>,
    pub processing: Option<f64>,
    pub synchronization: Option<f64>,
    pub local_disk_io: Option<f64>,
    pub remote_disk_io: Option<f64>,
    pub network_communication: Option<f64>,
}

impl QueryPlan {
    /// Sets each node's `estimated_ms` to its share of the query's total
    /// execution time (`EXECUTION_TIME` from `QUERY_HISTORY`).
    ///
    /// This attributes time; it does not measure it. Operators run in
    /// parallel and pipelined, so these are not wall-clock durations.
    pub fn with_execution_time(mut self, execution_ms: u64) -> Self {
        for node in self.steps.iter_mut().flat_map(|step| &mut step.nodes) {
            node.estimated_ms = node.time.overall.map(|share| share * execution_ms as f64);
        }
        self
    }
}

impl Step {
    /// Looks up a node by operator id.
    pub fn node(&self, id: u32) -> Option<&Node> {
        let index = self.nodes.binary_search_by_key(&id, |node| node.id).ok()?;
        Some(&self.nodes[index])
    }

    /// The nodes with no parents, usually a single `Result`.
    pub fn roots(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(|node| node.parents.is_empty())
    }

    /// The nodes that feed the node with the given id.
    pub fn children(&self, id: u32) -> impl Iterator<Item = &Node> {
        let child_ids = self
            .node(id)
            .map(|node| node.children.as_slice())
            .unwrap_or_default();
        child_ids.iter().filter_map(|&child| self.node(child))
    }
}
