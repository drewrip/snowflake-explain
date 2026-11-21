# snowflake-explain

A standard `EXPLAIN ANALYZE`-style plan for Snowflake queries.

Snowflake has no `EXPLAIN ANALYZE`. Its closest equivalent is
[`GET_QUERY_OPERATOR_STATS`](https://docs.snowflake.com/en/sql-reference/functions/get_query_operator_stats),
which returns one flat row per operator. This crate turns those rows into a
`QueryPlan`: a small, typed model that libraries and apps can share as Rust
values or as JSON.

```rust
use snowflake_explain::{QueryPlan, rows_from_json};

// JSON rows from: SELECT * FROM TABLE(GET_QUERY_OPERATOR_STATS('<query id>'))
let plan = QueryPlan::from_rows(rows_from_json(&json)?)?
    .with_execution_time(execution_ms); // optional: EXECUTION_TIME from QUERY_HISTORY

println!("{plan}");

// An estimated plan, for a query that has not run (or a view):
// the output of EXPLAIN USING JSON <query>
let estimate = QueryPlan::from_explain_json(&explain_json)?;
```

```text
Step 1
-> Result [0]  rows=10  time=0.4%
   -> SortWithLimit [1]  rows=1.2M->10  time=12.1%
      -> Join [2]  INNER  on (O.CUSTOMER_ID = C.ID)  and (O.TOTAL > C.CREDIT_LIMIT)  rows=3.4M->1.2M  spilled=1.1GB  time=40.2%
         -> TableScan [3]  rows=2.1M  partitions=12/980  time=30.0%
         -> TableScan [4]  rows=1.3M  partitions=40/40  time=17.3%
```

## The model

- **`QueryPlan`** has its `steps` (a query can have more than one), a `query_id` (`None` for an estimated plan), and optional query-level times: `execution_ms`, `elapsed_ms` and `compilation_ms`.
- **`Step`** has an optional `estimated_ms` and holds a flat list of `nodes`, sorted by operator id. The operators form a DAG: a CTE's `WithClause` can have several parents. Nodes therefore point to each other by id (`parents`, `children`), and a shared node is stored only once.
- **`Node`** has its `operator` name (exactly as Snowflake spells it), `stats`, `time`, its raw `attributes`, and an optional `estimated_ms`.
- **`Stats`** has typed fields for the common metrics: rows read and produced, rows inserted, updated and deleted, bytes scanned, partitions, spilling and network. Every other statistic is kept unchanged in `other`.
- **`TimeBreakdown`** gives Snowflake's per-operator share of execution time, as fractions where `1.0` is the whole **step**, not the whole query. Categories it has no field for are kept in `extra`.

Fields that are `None` or empty are left out of the JSON form.

## Time

Snowflake reports each operator's share of its step's time, but not how long each step took. A `CREATE TABLE … AS SELECT`, for example, has a step 1 holding a lone `CREATE TABLE` operator at 100%, and a step 2 whose operators again add up to about 100%.

`with_execution_time(ms)` therefore first splits the query's time among steps (`split_execution_time`): evenly among the steps with more than one operator that report any time; failing that, among any steps that report time; failing that, among all steps. Each operator then gets its share of its step. To split time differently, pass your own per-step times to `with_step_times`.

## Estimated plans

`QueryPlan::from_explain_json` reads `EXPLAIN USING JSON` into the same model, with no statistics or times. Operator names are kept as `EXPLAIN` gives them (`InnerJoin` where operator stats say `Join`), but arguments are rewritten into the attribute names `GET_QUERY_OPERATOR_STATS` uses, so the same code can read both: `aggExprs` → `functions`, `groupKeys` → `grouping_keys`, `joinKey` → `equality_join_condition`, `sortKey` → `sort_keys`, `rowCount` → `rows`, the join type → `join_type`, a filter → `filter_condition`, and a scan's object, columns and alias → `table_name`, `columns`, `table_alias`. Anything else is kept under its own name.

The library makes no connection to Snowflake. Fetch the rows with any driver and pass them in as JSON, or build `OperatorStatsRow` values directly. Both object-valued and string-encoded VARIANT columns are accepted.
