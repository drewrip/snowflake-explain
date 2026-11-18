use serde_json::json;
use snowflake_explain::{BuildError, OperatorStatsRow, QueryPlan, rows_from_json};

fn plan(fixture: &str) -> QueryPlan {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let json = std::fs::read_to_string(path).unwrap();
    QueryPlan::from_rows(rows_from_json(&json).unwrap()).unwrap()
}

fn row(
    query_id: &str,
    step_id: u32,
    operator_id: u32,
    parents: Option<Vec<u32>>,
) -> OperatorStatsRow {
    OperatorStatsRow {
        query_id: query_id.into(),
        step_id,
        operator_id,
        parent_operators: parents,
        operator_type: "Result".into(),
        operator_statistics: json!(null),
        execution_time_breakdown: json!(null),
        operator_attributes: json!(null),
    }
}

#[test]
fn join_plan_links_parents_and_children() {
    let plan = plan("join.json");
    let step = &plan.steps[0];

    assert_eq!(plan.query_id, "01b0-join");
    assert_eq!(step.roots().map(|n| n.id).collect::<Vec<_>>(), [0]);
    assert_eq!(step.node(2).unwrap().children, [3, 4]);
    assert_eq!(
        step.children(2)
            .map(|n| n.operator.as_str())
            .collect::<Vec<_>>(),
        ["TableScan", "TableScan"]
    );
}

#[test]
fn common_stats_are_typed_and_the_rest_is_kept() {
    let plan = plan("join.json");
    let scan = plan.steps[0].node(3).unwrap();

    assert_eq!(scan.stats.output_rows, Some(2_100_000));
    assert_eq!(scan.stats.bytes_scanned, Some(52_000_000));
    assert_eq!(scan.stats.partitions_scanned, Some(12));
    assert_eq!(scan.stats.partitions_total, Some(980));
    assert_eq!(scan.stats.network_bytes, Some(4096));
    // Extracted values are removed from `other`; emptied groups disappear.
    assert_eq!(
        json!(scan.stats.other),
        json!({"io": {"percentage_scanned_from_cache": 0.5}})
    );
    assert_eq!(scan.attributes["table_name"], "SHOP.PUBLIC.ORDERS");
    assert_eq!(scan.time.remote_disk_io, Some(0.3));
}

#[test]
fn shared_cte_node_is_stored_once_with_both_parents() {
    let plan = plan("cte_rest_api.json");
    let step = &plan.steps[0];

    assert_eq!(step.nodes.len(), 6);
    let with_clause = step.node(4).unwrap();
    assert_eq!(with_clause.parents, [2, 3]);
    assert_eq!(step.node(2).unwrap().children, [4]);
    assert_eq!(step.node(3).unwrap().children, [4]);
}

#[test]
fn unknown_operators_and_stats_pass_through() {
    let plan = plan("cte_rest_api.json");
    let node = plan.steps[0].node(5).unwrap();

    assert_eq!(node.operator, "SomeFutureOperator");
    assert_eq!(node.stats.bytes_scanned, Some(2048));
    assert_eq!(
        json!(node.stats.other),
        json!({"io": {"scan_progress": 1.0}, "brand_new_metric": {"widgets": 3}})
    );
    assert_eq!(node.time.overall, Some(0.6));
}

#[test]
fn steps_are_kept_separate_and_ordered() {
    let plan = plan("multi_step.json");

    assert_eq!(plan.steps.iter().map(|s| s.id).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(plan.steps[0].nodes.len(), 1);
    assert_eq!(plan.steps[1].nodes.len(), 2);
    assert_eq!(plan.steps[1].node(1).unwrap().operator, "Filter");
}

#[test]
fn execution_time_is_attributed_by_share() {
    let plan = plan("multi_step.json").with_execution_time(1000);

    assert_eq!(plan.steps[0].nodes[0].estimated_ms, Some(500.0));
    assert_eq!(plan.steps[1].nodes[1].estimated_ms, Some(300.0));
}

#[test]
fn json_round_trip_is_lossless() {
    let plan = plan("cte_rest_api.json").with_execution_time(42);
    let json = serde_json::to_string(&plan).unwrap();

    assert_eq!(serde_json::from_str::<QueryPlan>(&json).unwrap(), plan);
}

#[test]
fn renders_join_plan() {
    assert_eq!(
        plan("join.json").to_string(),
        "\
Step 1
-> Result [0]  rows=10  time=0.4%
   -> SortWithLimit [1]  rows=1.2M->10  time=12.1%
      -> Join [2]  INNER  on (O.CUSTOMER_ID = C.ID)  and (O.TOTAL > C.CREDIT_LIMIT)  rows=3.4M->1.2M  spilled=1.1GB  time=40.2%
         -> TableScan [3]  rows=2.1M  partitions=12/980  time=30.0%
         -> TableScan [4]  rows=1.3M  partitions=40/40  time=17.3%
"
    );
}

#[test]
fn renders_shared_node_once() {
    assert_eq!(
        plan("cte_rest_api.json").to_string(),
        "\
Step 1
-> Result [0]  rows=20  time=10.0%
   -> UnionAll [1]  rows=20  time=10.0%
      -> WithReference [2]  rows=10  time=0.0%
         -> WithClause [4]  rows=10  time=20.0%
            -> SomeFutureOperator [5]  rows=10  time=60.0%
      -> WithReference [3]  rows=10  time=0.0%
         -> WithClause [4]  (see above)
"
    );
}

#[test]
fn renders_filter_predicate_and_each_step() {
    assert_eq!(
        plan("multi_step.json").to_string(),
        "\
Step 1
-> Aggregate [0]  rows=50->1  time=50.0%
Step 2
-> Result [0]  rows=1  time=20.0%
   -> Filter [1]  where T.X > (SELECT MAX(Y) FROM U)  rows=100->1  time=30.0%
"
    );
}

#[test]
fn rejects_empty_input() {
    assert_eq!(QueryPlan::from_rows(vec![]), Err(BuildError::Empty));
}

#[test]
fn rejects_mixed_query_ids() {
    let rows = vec![row("a", 1, 0, None), row("b", 1, 1, Some(vec![0]))];
    assert_eq!(
        QueryPlan::from_rows(rows),
        Err(BuildError::MixedQueryIds("a".into(), "b".into()))
    );
}

#[test]
fn rejects_duplicate_operators() {
    let rows = vec![row("a", 1, 0, None), row("a", 1, 0, None)];
    assert_eq!(
        QueryPlan::from_rows(rows),
        Err(BuildError::DuplicateOperator {
            step: 1,
            operator: 0
        })
    );
}

#[test]
fn rejects_unknown_parents() {
    let rows = vec![row("a", 1, 0, None), row("a", 1, 1, Some(vec![7]))];
    assert_eq!(
        QueryPlan::from_rows(rows),
        Err(BuildError::UnknownParent {
            step: 1,
            operator: 1,
            parent: 7
        })
    );
}
