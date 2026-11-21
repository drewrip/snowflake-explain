use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The plan of one Snowflake query.
///
/// Built from `GET_QUERY_OPERATOR_STATS` ([`QueryPlan::from_rows`]) it is the
/// executed plan, Snowflake's answer to `EXPLAIN ANALYZE`. Built from
/// `EXPLAIN USING JSON` ([`QueryPlan::from_explain_json`]) it is the estimated
/// plan: the same shape, with no statistics or times.
///
/// Fields that are `None` or empty are left out of the JSON form.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QueryPlan {
    /// The query the plan was executed by. `None` for an estimated plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_id: Option<String>,
    /// The query's `EXECUTION_TIME` from `QUERY_HISTORY`, in milliseconds.
    /// Set by [`QueryPlan::with_execution_time`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_ms: Option<u64>,
    /// The query's `TOTAL_ELAPSED_TIME` (wall time), in milliseconds, if the caller has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    /// The query's `COMPILATION_TIME`, in milliseconds, if the caller has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compilation_ms: Option<u64>,
    /// Ordered by step id.
    pub steps: Vec<Step>,
}

/// One step of a query. Each step has its own graph of operators.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub id: u32,
    /// Estimated milliseconds of the query's execution time spent in this
    /// step. Set by [`QueryPlan::with_execution_time`] or
    /// [`QueryPlan::with_step_times`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_ms: Option<f64>,
    /// Ordered by operator id.
    pub nodes: Vec<Node>,
}

/// One operator in a step.
///
/// Operators form a DAG, not a tree: a shared operator (such as the
/// `WithClause` behind a CTE) can have several parents. Nodes refer to each
/// other by operator id, so a shared node is stored exactly once.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Operator id, unique within the step.
    pub id: u32,
    /// Operator type exactly as Snowflake names it, e.g. `TableScan` or `Join`.
    pub operator: String,
    /// Operators that consume this one's output. Empty for the root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<u32>,
    /// Operators that feed this one: the inverse of `parents`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<u32>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub stats: Stats,
    #[serde(default, skip_serializing_if = "is_default")]
    pub time: TimeBreakdown,
    /// Type-specific details such as `table_name` or `join_type`, kept as Snowflake gives them.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub attributes: Map<String, Value>,
    /// Estimated milliseconds attributed to this operator: its share of its
    /// step's [`Step::estimated_ms`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_ms: Option<f64>,
}

/// The commonly needed operator statistics, plus everything else Snowflake reported.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_rows: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_rows: Option<u64>,
    /// Rows written by a DML operator (`dml.number_of_rows_inserted`). For a
    /// `CREATE TABLE … AS SELECT` this is the new table's row count; the
    /// writing operator itself reports `output_rows: 0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_inserted: Option<u64>,
    /// `dml.number_of_rows_updated`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_updated: Option<u64>,
    /// `dml.number_of_rows_deleted`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_deleted: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_scanned: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partitions_scanned: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partitions_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_spilled_local: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_spilled_remote: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_bytes: Option<u64>,
    /// The remaining statistics, in Snowflake's original nested layout.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub other: Map<String, Value>,
}

/// An operator's share of its **step's** execution time, as reported by
/// Snowflake in `EXECUTION_TIME_BREAKDOWN`.
///
/// Shares are fractions (not 0–100 percentages), and they sum to about 1.0
/// within each step, not across the whole query. Snowflake does not report
/// how long each step took, so a share alone cannot be turned into time;
/// see [`QueryPlan::with_execution_time`].
///
/// `overall` is the operator's total share; the other fields split it by activity.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeBreakdown {
    #[serde(alias = "overall_percentage", skip_serializing_if = "Option::is_none")]
    pub overall: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialization: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synchronization: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_disk_io: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_disk_io: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_communication: Option<f64>,
    /// Time Snowflake puts in no other category, e.g. the whole of a
    /// `CREATE TABLE` step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<f64>,
    /// Categories this crate does not know yet, kept as Snowflake gives them.
    #[serde(flatten, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

impl QueryPlan {
    /// Records the query's execution time (`EXECUTION_TIME` from
    /// `QUERY_HISTORY`) and attributes it to steps and operators.
    ///
    /// Snowflake reports each operator's share of its step, but not how long
    /// each step took, so the time is first split among steps by
    /// [`QueryPlan::split_execution_time`], then each operator gets its share
    /// of its step. Use [`QueryPlan::with_step_times`] to split it differently.
    ///
    /// This attributes time; it does not measure it. Operators run in
    /// parallel and pipelined, so these are not wall-clock durations.
    pub fn with_execution_time(mut self, execution_ms: u64) -> Self {
        let step_times = self.split_execution_time(execution_ms);
        self.execution_ms = Some(execution_ms);
        self.with_step_times(step_times)
    }

    /// Sets each listed step's `estimated_ms`, and each of its operators'
    /// `estimated_ms` to the operator's share of that time. Takes
    /// `(step id, milliseconds)` pairs; steps not listed are left without
    /// estimates.
    pub fn with_step_times(mut self, step_times: impl IntoIterator<Item = (u32, f64)>) -> Self {
        for step in &mut self.steps {
            step.estimated_ms = None;
            for node in &mut step.nodes {
                node.estimated_ms = None;
            }
        }
        for (id, ms) in step_times {
            let Some(step) = self.steps.iter_mut().find(|step| step.id == id) else {
                continue;
            };
            step.estimated_ms = Some(ms);
            for node in &mut step.nodes {
                node.estimated_ms = node.time.overall.map(|share| share * ms);
            }
        }
        self
    }

    /// The default guess at how a query's execution time divides among its
    /// steps, as `(step id, milliseconds)` pairs. The time is split evenly
    /// among the steps that did measurable work, which are, in order of
    /// preference:
    ///
    /// 1. steps with more than one operator that report a nonzero share;
    /// 2. otherwise, any step that reports a nonzero share;
    /// 3. otherwise (nothing ran long enough to be measured), every step.
    ///
    /// A step of a single operator, such as the `CREATE TABLE` step of a
    /// `CREATE TABLE … AS SELECT`, always reports a share of 1.0 and so says
    /// nothing about where time went. It gets time only when no other step
    /// reports any. Steps that get none of the time are listed with 0.
    pub fn split_execution_time(&self, execution_ms: u64) -> Vec<(u32, f64)> {
        let busy = |step: &&Step| step.share_total() > 0.0;
        let mut chosen: Vec<u32> = self
            .steps
            .iter()
            .filter(busy)
            .filter(|step| step.nodes.len() > 1)
            .map(|step| step.id)
            .collect();
        if chosen.is_empty() {
            chosen = self.steps.iter().filter(busy).map(|step| step.id).collect();
        }
        if chosen.is_empty() {
            chosen = self.steps.iter().map(|step| step.id).collect();
        }
        let each = execution_ms as f64 / chosen.len().max(1) as f64;
        self.steps
            .iter()
            .map(|step| (step.id, if chosen.contains(&step.id) { each } else { 0.0 }))
            .collect()
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

    /// The sum of the operators' `overall` shares: about 1.0 for a step
    /// whose time Snowflake measured, 0.0 for one too quick to measure.
    pub fn share_total(&self) -> f64 {
        self.nodes.iter().filter_map(|node| node.time.overall).sum()
    }
}
