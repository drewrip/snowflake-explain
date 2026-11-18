use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::{BuildError, Node, OperatorStatsRow, QueryPlan, Stats, Step};

impl QueryPlan {
    /// Builds a plan from the rows of `GET_QUERY_OPERATOR_STATS` for one query.
    pub fn from_rows(rows: Vec<OperatorStatsRow>) -> Result<Self, BuildError> {
        let query_id = single_query_id(&rows)?;

        let mut nodes_by_step: BTreeMap<u32, Vec<Node>> = BTreeMap::new();
        for row in rows {
            nodes_by_step
                .entry(row.step_id)
                .or_default()
                .push(to_node(row));
        }

        let steps = nodes_by_step
            .into_iter()
            .map(|(id, nodes)| link_step(id, nodes))
            .collect::<Result<_, _>>()?;

        Ok(QueryPlan { query_id, steps })
    }
}

fn single_query_id(rows: &[OperatorStatsRow]) -> Result<String, BuildError> {
    let first = rows.first().ok_or(BuildError::Empty)?;
    match rows.iter().find(|row| row.query_id != first.query_id) {
        Some(other) => Err(BuildError::MixedQueryIds(
            first.query_id.clone(),
            other.query_id.clone(),
        )),
        None => Ok(first.query_id.clone()),
    }
}

/// Sorts a step's nodes and fills in each node's `children` from its children's `parents`.
fn link_step(id: u32, mut nodes: Vec<Node>) -> Result<Step, BuildError> {
    nodes.sort_by_key(|node| node.id);
    if let Some(pair) = nodes.windows(2).find(|pair| pair[0].id == pair[1].id) {
        return Err(BuildError::DuplicateOperator {
            step: id,
            operator: pair[0].id,
        });
    }

    // Children are visited in id order, so every `children` list ends up sorted.
    let links: Vec<(u32, u32)> = nodes
        .iter()
        .flat_map(|child| child.parents.iter().map(|&parent| (parent, child.id)))
        .collect();
    for (parent, child) in links {
        let index = nodes
            .binary_search_by_key(&parent, |node| node.id)
            .map_err(|_| BuildError::UnknownParent {
                step: id,
                operator: child,
                parent,
            })?;
        nodes[index].children.push(child);
    }

    Ok(Step { id, nodes })
}

fn to_node(row: OperatorStatsRow) -> Node {
    Node {
        id: row.operator_id,
        operator: row.operator_type,
        parents: row.parent_operators.unwrap_or_default(),
        children: Vec::new(),
        stats: to_stats(row.operator_statistics),
        time: serde_json::from_value(row.execution_time_breakdown).unwrap_or_default(),
        attributes: into_object(row.operator_attributes),
        estimated_ms: None,
    }
}

/// Moves the common statistics into typed fields and leaves the rest in `other`.
fn to_stats(statistics: Value) -> Stats {
    let mut other = into_object(statistics);
    Stats {
        input_rows: take_u64(&mut other, &["input_rows"]),
        output_rows: take_u64(&mut other, &["output_rows"]),
        bytes_scanned: take_u64(&mut other, &["io", "bytes_scanned"]),
        partitions_scanned: take_u64(&mut other, &["pruning", "partitions_scanned"]),
        partitions_total: take_u64(&mut other, &["pruning", "partitions_total"]),
        bytes_spilled_local: take_u64(&mut other, &["spilling", "bytes_spilled_local_storage"]),
        bytes_spilled_remote: take_u64(&mut other, &["spilling", "bytes_spilled_remote_storage"]),
        network_bytes: take_u64(&mut other, &["network", "network_bytes"]),
        other,
    }
}

/// Removes the number at `path` (e.g. `["io", "bytes_scanned"]`) from `map`,
/// dropping any nested object that becomes empty. Non-numbers are left in place.
fn take_u64(map: &mut Map<String, Value>, path: &[&str]) -> Option<u64> {
    match path {
        [key] => {
            let number = map.get(*key)?.as_f64()?;
            map.remove(*key);
            Some(number as u64)
        }
        [group, rest @ ..] => {
            let inner = map.get_mut(*group)?.as_object_mut()?;
            let number = take_u64(inner, rest);
            if inner.is_empty() {
                map.remove(*group);
            }
            number
        }
        [] => None,
    }
}

fn into_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
