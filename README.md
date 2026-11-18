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

- **`QueryPlan`** has a `query_id` and its `steps`. A query can have more than one step.
- **`Step`** holds a flat list of `nodes`, sorted by operator id. The operators form a DAG: a CTE's `WithClause` can have several parents. Nodes therefore point to each other by id (`parents`, `children`), and a shared node is stored only once.
- **`Node`** has its `operator` name (exactly as Snowflake spells it), `stats`, `time`, its raw `attributes`, and an optional `estimated_ms`.
- **`Stats`** has typed fields for the common metrics: rows, bytes scanned, partitions, spilling and network. Every other statistic is kept unchanged in `other`.
- **`TimeBreakdown`** gives Snowflake's per-operator share of execution time, as fractions where `1.0` is the whole query.

The library makes no connection to Snowflake. Fetch the rows with any driver and pass them in as JSON, or build `OperatorStatsRow` values directly. Both object-valued and string-encoded VARIANT columns are accepted.
