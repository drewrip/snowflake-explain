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

    assert_eq!(plan.query_id.as_deref(), Some("01b0-join"));
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

// The fixtures below are real GET_QUERY_OPERATOR_STATS output from Snowflake,
// exported with `snow sql --format json`, for queries over synthetic
// GENERATOR data. Only the query ids were replaced.

#[test]
fn snowflake_join_filter() {
    let plan = plan("snowflake_join_filter.json");
    let join = plan.steps[0].node(2).unwrap();

    assert_eq!(join.operator, "Join");
    assert_eq!(join.attributes["join_type"], "INNER");
    assert_eq!(join.children, [3, 4]);
    assert_eq!(
        plan.to_string(),
        "\
Step 1
-> Result [0]  rows=10  time=0.0%
   -> SortWithLimit [1]  rows=736->10  time=0.0%
      -> Join [2]  INNER  on (SEQ4() = UNIFORM(1, 100, RANDOM(1)))  and (UNIFORM(1, 1000, RANDOM(2))) > (UNIFORM(1, 1000, RANDOM(3)))  rows=1.5K->736  time=0.0%
         -> Generator [3]  rows=100  time=0.0%
         -> Filter [4]  where (SEQ4() % 7) = 0  rows=9.9K->1.4K  time=0.0%
            -> JoinFilter [5]  rows=10.0K->9.9K  time=0.0%
               -> Generator [6]  rows=10.0K  time=0.0%
"
    );
}

#[test]
fn snowflake_shared_cte() {
    let plan = plan("snowflake_shared_cte.json");
    let step = &plan.steps[0];
    let with_clause = step.node(5).unwrap();

    assert_eq!(with_clause.operator, "WithClause");
    assert_eq!(with_clause.parents, [4, 10]);
    assert_eq!(
        step.nodes
            .iter()
            .filter(|n| n.operator == "WithClause")
            .count(),
        1
    );
    assert!(plan.to_string().contains("-> WithClause [5]  (see above)"));
}

#[test]
fn snowflake_scalar_subquery_runs_as_two_steps() {
    let plan = plan("snowflake_scalar_subquery.json");

    assert_eq!(plan.steps.iter().map(|s| s.id).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(plan.steps[0].nodes.len(), 3);
    assert_eq!(
        plan.steps[1].node(2).unwrap().attributes["filter_condition"],
        "(UNIFORM(1, 10, RANDOM(4))) > Subquery(Step1)"
    );
}

/// A 100M-row query that ran for 3227 ms. Its time shares sum to about 1,
/// which shows Snowflake reports them as fractions, not 0-100 percentages.
#[test]
fn snowflake_time_shares_are_fractions() {
    let plan = plan("snowflake_time_shares.json").with_execution_time(3227);
    let step = &plan.steps[0];
    let total: f64 = step.nodes.iter().filter_map(|n| n.time.overall).sum();

    assert!((0.9..=1.0).contains(&total), "shares sum to {total}");
    let filter = step.node(3).unwrap();
    assert_eq!(filter.operator, "Filter");
    assert_eq!(filter.estimated_ms.unwrap().round(), 2347.0);
    assert!(plan.to_string().contains("-> Filter [3]  where ((UNIFORM(1, 1000000, RANDOM(1))) % 3) = 0  rows=100.0M->33.3M  time=72.7%  ~2347ms"));
}

// A real `CREATE TRANSIENT TABLE … AS SELECT` that ran for 1135 ms, from dee.
// Its raw rows were rebuilt from the plan dee stored; the `other` time share
// of `CREATE TABLE`, which older versions of this crate dropped, was restored.

#[test]
fn snowflake_ctas_runs_as_two_steps() {
    let plan = plan("snowflake_ctas.json");

    assert_eq!(plan.steps.iter().map(|s| s.id).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(plan.steps[0].nodes.len(), 1);
    assert_eq!(plan.steps[0].nodes[0].operator, "CREATE TABLE");
    assert_eq!(plan.steps[1].nodes.len(), 8);
}

/// Shares add up to about 1 within each step, not across the query.
#[test]
fn snowflake_time_shares_are_per_step() {
    let plan = plan("snowflake_ctas.json");
    for step in &plan.steps {
        let total = step.share_total();
        assert!(
            (0.9..=1.0).contains(&total),
            "step {} sums to {total}",
            step.id
        );
    }
}

#[test]
fn execution_time_is_not_counted_twice() {
    let plan = plan("snowflake_ctas.json").with_execution_time(1135);

    assert_eq!(plan.execution_ms, Some(1135));
    // The lone `CREATE TABLE` step says nothing about time; the query step gets it all.
    assert_eq!(plan.steps[0].estimated_ms, Some(0.0));
    assert_eq!(plan.steps[0].nodes[0].estimated_ms, Some(0.0));
    assert_eq!(plan.steps[1].estimated_ms, Some(1135.0));
    let total: f64 = plan
        .steps
        .iter()
        .flat_map(|s| &s.nodes)
        .filter_map(|n| n.estimated_ms)
        .sum();
    assert!(total <= 1135.0, "attributed {total} ms");
    let scan = plan.steps[1].node(7).unwrap();
    assert_eq!(scan.estimated_ms.unwrap().round(), 378.0);
}

#[test]
fn time_goes_to_the_creation_step_when_the_query_was_too_quick() {
    let mut plan = plan("snowflake_ctas.json");
    for node in &mut plan.steps[1].nodes {
        node.time.overall = Some(0.0);
    }

    assert_eq!(plan.split_execution_time(100), [(1, 100.0), (2, 0.0)]);
}

#[test]
fn time_is_split_evenly_among_measured_steps() {
    let plan = plan("snowflake_scalar_subquery.json");
    // Too quick to measure: every step reports zero shares.
    assert_eq!(plan.split_execution_time(100), [(1, 50.0), (2, 50.0)]);

    let mut plan = plan;
    plan.steps[0].nodes[1].time.overall = Some(1.0);
    plan.steps[1].nodes[2].time.overall = Some(1.0);
    assert_eq!(plan.split_execution_time(100), [(1, 50.0), (2, 50.0)]);
}

#[test]
fn step_times_can_be_given_directly() {
    let plan = plan("snowflake_ctas.json").with_step_times([(2, 1000.0)]);

    assert_eq!(plan.execution_ms, None);
    assert_eq!(plan.steps[0].estimated_ms, None);
    assert_eq!(plan.steps[0].nodes[0].estimated_ms, None);
    assert_eq!(
        plan.steps[1].node(2).unwrap().estimated_ms.unwrap().round(),
        303.0
    );
}

#[test]
fn time_breakdown_keeps_other_and_unknown_categories() {
    let plan = plan("snowflake_ctas.json");
    assert_eq!(plan.steps[0].nodes[0].time.other, Some(1.0));

    let rows = rows_from_json(
        r#"[{"QUERY_ID": "q", "STEP_ID": 1, "OPERATOR_ID": 0, "OPERATOR_TYPE": "Result",
             "EXECUTION_TIME_BREAKDOWN": {"overall_percentage": 0.5, "processing": 0.2,
                                          "brand_new_category": 0.3}}]"#,
    )
    .unwrap();
    let time = &QueryPlan::from_rows(rows).unwrap().steps[0].nodes[0].time;
    assert_eq!(time.overall, Some(0.5));
    assert_eq!(time.processing, Some(0.2));
    assert_eq!(json!(time.extra), json!({"brand_new_category": 0.3}));
    assert_eq!(
        serde_json::to_value(time).unwrap(),
        json!({"overall": 0.5, "processing": 0.2, "brand_new_category": 0.3})
    );
}

#[test]
fn dml_row_counts_are_typed() {
    let plan = plan("snowflake_ctas.json");
    let write = plan.steps[1].node(1).unwrap();

    assert_eq!(write.stats.rows_inserted, Some(1000));
    assert_eq!(write.stats.output_rows, Some(0));
    assert_eq!(
        json!(write.stats.other),
        json!({"io": {"bytes_written": 15872}})
    );
}

#[test]
fn renders_steps_with_their_time() {
    assert_eq!(
        plan("snowflake_ctas.json")
            .with_execution_time(1135)
            .to_string(),
        "\
Step 1  ~0ms
-> CREATE TABLE [0]  time=100.0%  ~0ms
Step 2  ~1135ms
-> Result [0]  rows=0  time=0.0%  ~0ms
   -> CreateTableAsSelect [1]  rows=1.0K->0  inserted=1.0K  time=24.2%  ~275ms
      -> Aggregate [2]  rows=6.7M->1.0K  time=30.3%  ~344ms
         -> Join [3]  INNER  on (T.K = B.K % 7)  rows=6.7M->6.7M  time=0.0%  ~0ms
            -> TableScan [4]  rows=7  partitions=1/1  time=0.0%  ~0ms
            -> Filter [5]  where (B.A % 3) = 0  rows=6.7M  time=0.0%  ~0ms
               -> JoinFilter [6]  rows=6.7M  time=3.0%  ~34ms
                  -> TableScan [7]  rows=6.7M  partitions=20/20  time=33.3%  ~378ms
"
    );
}

#[test]
fn empty_fields_are_left_out_of_json() {
    let plan = plan("snowflake_ctas.json");
    let json = serde_json::to_string(&plan).unwrap();

    assert!(!json.contains("null"), "{json}");
    assert_eq!(
        serde_json::to_value(&plan.steps[1].nodes[0]).unwrap(),
        json!({"id": 0, "operator": "Result", "children": [1],
               "stats": {"input_rows": 0}, "time": {"overall": 0.0}})
    );
    assert_eq!(serde_json::from_str::<QueryPlan>(&json).unwrap(), plan);
}

/// Plans written by version 0.1, with every field present and nulls, still load.
#[test]
fn reads_plans_written_with_nulls() {
    let old = r#"{"query_id": "q", "steps": [{"id": 1, "nodes": [
        {"id": 0, "operator": "Result", "parents": [], "children": [],
         "stats": {"input_rows": 5, "output_rows": null, "other": {}},
         "time": {"overall": 1.0, "processing": null},
         "attributes": {}, "estimated_ms": null}]}]}"#;
    let plan: QueryPlan = serde_json::from_str(old).unwrap();

    assert_eq!(plan.query_id.as_deref(), Some("q"));
    assert_eq!(plan.steps[0].nodes[0].stats.input_rows, Some(5));
    assert_eq!(plan.steps[0].nodes[0].time.overall, Some(1.0));
}

// `EXPLAIN USING JSON` output for the same queries as dee's fixtures.

fn explained(fixture: &str) -> QueryPlan {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    QueryPlan::from_explain_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn explain_uses_operator_stats_attributes() {
    let plan = explained("snowflake_explain_big.json");
    let step = &plan.steps[0];

    assert_eq!(plan.query_id, None);
    assert_eq!(step.roots().map(|n| n.id).collect::<Vec<_>>(), [0]);
    assert_eq!(step.node(2).unwrap().children, [3, 4]);
    assert_eq!(
        json!(step.node(1).unwrap().attributes),
        json!({"functions": ["SUM(B.A)", "COUNT(DISTINCT B.S)", "MAX(T.N)"],
               "grouping_keys": ["B.K"]})
    );
    assert_eq!(
        json!(step.node(2).unwrap().attributes),
        json!({"equality_join_condition": "(T.K = B.K % 7)", "join_type": "INNER"})
    );
    assert_eq!(
        json!(step.node(3).unwrap().attributes),
        json!({"table_name": "CMU.DAGBENCH_DEE_SMOKE.T", "columns": ["K", "N"],
               "table_alias": "T", "partitionsAssigned": 1, "partitionsTotal": 1,
               "bytesAssigned": 1024})
    );
    assert_eq!(
        step.node(4).unwrap().attributes["filter_condition"],
        "(B.A % 3) = 0"
    );
    assert_eq!(step.node(0).unwrap().stats, Default::default());
    assert_eq!(step.node(0).unwrap().time, Default::default());
}

#[test]
fn explain_sort_and_outer_join() {
    let plan = explained("snowflake_explain_left_join_limit.json");
    let step = &plan.steps[0];

    assert_eq!(
        json!(step.node(1).unwrap().attributes),
        json!({"sort_keys": ["S.A ASC NULLS LAST"], "rows": 100.0})
    );
    assert_eq!(step.node(2).unwrap().attributes["join_type"], "LEFT OUTER");
}

#[test]
fn explain_renders_like_a_profile() {
    assert_eq!(
        explained("snowflake_explain_shared.json").to_string(),
        "\
Step 1
-> Result [0]
   -> WindowFunction [1]
      -> InnerJoin [2]  INNER  on ((Y.K + 1) % 7 = X.K)
         -> WithReference [3]
            -> WithClause [4]
               -> Aggregate [5]
                  -> Filter [6]  where SRC.A > 10
                     -> TableScan [7]
         -> JoinFilter [8]  on ((Y.K + 1) % 7 = X.K)
            -> WithReference [9]
               -> WithClause [4]  (see above)
"
    );
    let plan = explained("snowflake_explain_shared.json");
    assert_eq!(plan.steps[0].node(4).unwrap().attributes["name"], "C");
    assert_eq!(plan.steps[0].node(9).unwrap().attributes["alias"], "X");
}

#[test]
fn explain_rejects_other_documents() {
    assert!(matches!(
        QueryPlan::from_explain_json(r#"{"rows": []}"#),
        Err(BuildError::InvalidExplain(_))
    ));
    assert!(matches!(
        QueryPlan::from_explain_json("not json"),
        Err(BuildError::InvalidExplain(_))
    ));
    assert_eq!(
        QueryPlan::from_explain_json(r#"{"Operations": []}"#),
        Err(BuildError::Empty)
    );
}
