use std::collections::HashSet;
use std::fmt;

use crate::{Node, QueryPlan, Step};

/// Prints the plan as an indented tree, one line per operator. `time=` is
/// the operator's share of its step:
///
/// ```text
/// Step 1
/// -> Result [0]  rows=10  time=0.4%
///    -> Join [1]  INNER  on (O.CUSTOMER_ID = C.ID)  rows=3.4M->1.2M  spilled=1.1GB  time=40.2%
///       -> TableScan [2]  rows=2.1M  partitions=12/980  time=30.0%
/// ```
///
/// A node with several parents is printed in full once; later visits print
/// `(see above)` instead of repeating its subtree.
impl fmt::Display for QueryPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for step in &self.steps {
            match step.estimated_ms {
                Some(ms) => writeln!(f, "Step {}  ~{ms:.0}ms", step.id)?,
                None => writeln!(f, "Step {}", step.id)?,
            }
            let mut printed = HashSet::new();
            for root in step.roots() {
                write_node(f, step, root, 0, &mut printed)?;
            }
        }
        Ok(())
    }
}

fn write_node(
    f: &mut fmt::Formatter<'_>,
    step: &Step,
    node: &Node,
    depth: usize,
    printed: &mut HashSet<u32>,
) -> fmt::Result {
    let indent = "   ".repeat(depth);
    if !printed.insert(node.id) {
        return writeln!(f, "{indent}-> {} [{}]  (see above)", node.operator, node.id);
    }

    writeln!(
        f,
        "{indent}-> {} [{}]{}",
        node.operator,
        node.id,
        summary(node)
    )?;
    for child in step.children(node.id) {
        write_node(f, step, child, depth + 1, printed)?;
    }
    Ok(())
}

/// Attributes that hold an operator's join type or predicates, with the label
/// printed before each. Matched by attribute, so any operator that has them shows them.
const PREDICATES: [(&str, &str); 4] = [
    ("join_type", ""),
    ("equality_join_condition", "on "),
    ("additional_join_condition", "and "),
    ("filter_condition", "where "),
];

/// The most useful facts about a node, e.g. `  INNER  on (A.ID = B.ID)  rows=3.4M->1.2M  time=40.2%`.
fn summary(node: &Node) -> String {
    let stats = &node.stats;
    let mut parts = Vec::new();

    for (key, label) in PREDICATES {
        if let Some(text) = node.attributes.get(key).and_then(|v| v.as_str())
            && !text.is_empty()
        {
            parts.push(format!("{label}{text}"));
        }
    }
    match (stats.input_rows, stats.output_rows) {
        (Some(input), Some(output)) if input != output => {
            parts.push(format!("rows={}->{}", compact(input), compact(output)));
        }
        // `Result` reports only input rows, sources only output rows.
        (input, output) => {
            if let Some(rows) = output.or(input) {
                parts.push(format!("rows={}", compact(rows)));
            }
        }
    }
    for (label, rows) in [
        ("inserted", stats.rows_inserted),
        ("updated", stats.rows_updated),
        ("deleted", stats.rows_deleted),
    ] {
        if let Some(rows) = rows {
            parts.push(format!("{label}={}", compact(rows)));
        }
    }
    if let (Some(scanned), Some(total)) = (stats.partitions_scanned, stats.partitions_total) {
        parts.push(format!("partitions={scanned}/{total}"));
    }
    let spilled = stats.bytes_spilled_local.unwrap_or(0) + stats.bytes_spilled_remote.unwrap_or(0);
    if spilled > 0 {
        parts.push(format!("spilled={}B", compact(spilled)));
    }
    if let Some(share) = node.time.overall {
        parts.push(format!("time={:.1}%", share * 100.0));
    }
    if let Some(ms) = node.estimated_ms {
        parts.push(format!("~{ms:.0}ms"));
    }

    parts.iter().map(|part| format!("  {part}")).collect()
}

/// Shortens large numbers: 950, 3.4K, 1.2M, 7.0G.
fn compact(n: u64) -> String {
    const UNITS: [(f64, &str); 4] = [(1e12, "T"), (1e9, "G"), (1e6, "M"), (1e3, "K")];
    let value = n as f64;
    match UNITS.iter().find(|(size, _)| value >= *size) {
        Some((size, unit)) => format!("{:.1}{unit}", value / size),
        None => n.to_string(),
    }
}
