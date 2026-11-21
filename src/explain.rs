use serde_json::{Map, Value};

use crate::build::link_step;
use crate::{BuildError, Node, QueryPlan};

/// `EXPLAIN` writes some arguments as `label: value`. These are the labels
/// whose meaning `GET_QUERY_OPERATOR_STATS` reports as an attribute, with
/// that attribute's name and form.
const LABELS: [(&str, &str, Form); 5] = [
    ("aggExprs", "functions", Form::List),
    ("groupKeys", "grouping_keys", Form::List),
    ("joinKey", "equality_join_condition", Form::Text),
    ("sortKey", "sort_keys", Form::List),
    ("rowCount", "rows", Form::Number),
];

#[derive(Clone, Copy)]
enum Form {
    List,
    Text,
    Number,
}

impl QueryPlan {
    /// Builds an estimated plan from the output of `EXPLAIN USING JSON <query>`.
    ///
    /// The plan has the same shape as one from [`QueryPlan::from_rows`], so
    /// the same code can read both. It has no `query_id`, statistics or
    /// times. Operator names are kept as `EXPLAIN` gives them (it says
    /// `InnerJoin` where operator stats say `Join`), but arguments are
    /// rewritten into the attributes `GET_QUERY_OPERATOR_STATS` reports for
    /// the same operator: `aggExprs` become `functions`, `groupKeys`
    /// `grouping_keys`, `joinKey` `equality_join_condition`, `sortKey`
    /// `sort_keys`, `rowCount` `rows`; a join's type becomes `join_type`; a
    /// filter's expression `filter_condition`; a scan's object, columns and
    /// alias `table_name`, `columns` and `table_alias`. Anything else is kept:
    /// unrecognised expressions under `expressions`, and other fields (such as
    /// `partitionsAssigned`) under their own names.
    pub fn from_explain_json(json: &str) -> Result<Self, BuildError> {
        let doc: Value =
            serde_json::from_str(json).map_err(|e| BuildError::InvalidExplain(e.to_string()))?;
        Self::from_explain(&doc)
    }

    /// Like [`QueryPlan::from_explain_json`], for an already parsed document.
    pub fn from_explain(doc: &Value) -> Result<Self, BuildError> {
        if let Value::String(text) = doc {
            return Self::from_explain_json(text);
        }
        let invalid = |what: &str| BuildError::InvalidExplain(what.to_string());
        let operations = doc
            .get("Operations")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("no `Operations` array"))?;
        if operations
            .iter()
            .all(|ops| ops.as_array().is_none_or(Vec::is_empty))
        {
            return Err(BuildError::Empty);
        }

        let mut steps = Vec::new();
        for (index, ops) in operations.iter().enumerate() {
            let ops = ops
                .as_array()
                .ok_or_else(|| invalid("a step of `Operations` is not an array"))?;
            let nodes = ops.iter().map(to_node).collect::<Result<_, _>>()?;
            steps.push(link_step(index as u32 + 1, nodes)?);
        }
        Ok(QueryPlan {
            steps,
            ..QueryPlan::default()
        })
    }
}

fn to_node(op: &Value) -> Result<Node, BuildError> {
    let invalid = |what: &str| BuildError::InvalidExplain(format!("{what}: {op}"));
    let mut fields = op
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("operator is not an object"))?;
    let id = fields
        .remove("id")
        .as_ref()
        .and_then(Value::as_u64)
        .and_then(|id| u32::try_from(id).ok())
        .ok_or_else(|| invalid("operator has no numeric `id`"))?;
    let operator = match fields.remove("operation") {
        Some(Value::String(name)) => name,
        _ => return Err(invalid("operator has no `operation`")),
    };
    let parents = match fields.remove("parentOperators") {
        None | Some(Value::Null) => Vec::new(),
        Some(value) => serde_json::from_value(value)
            .map_err(|_| invalid("`parentOperators` is not a list of ids"))?,
    };
    let expressions: Vec<String> = match fields.remove("expressions") {
        Some(Value::Array(items)) => items
            .into_iter()
            .map(|item| match item {
                Value::String(text) => text,
                other => other.to_string(),
            })
            .collect(),
        Some(Value::String(text)) => vec![text],
        _ => Vec::new(),
    };

    let mut attributes = Map::new();
    if operator.ends_with("Scan") {
        scan_attributes(&mut attributes, &mut fields, expressions);
    } else {
        expression_attributes(&mut attributes, &operator, expressions);
    }
    if let Some(join_type) = join_type(&operator) {
        attributes.insert("join_type".into(), join_type.into());
    }
    // Whatever has no counterpart in operator stats is kept as it is.
    for (key, value) in fields {
        attributes.entry(key).or_insert(value);
    }

    Ok(Node {
        id,
        operator,
        parents,
        attributes,
        ..Node::default()
    })
}

fn scan_attributes(
    attributes: &mut Map<String, Value>,
    fields: &mut Map<String, Value>,
    columns: Vec<String>,
) {
    if let Some(Value::Array(objects)) = fields.get("objects")
        && let [Value::String(table)] = objects.as_slice()
    {
        attributes.insert("table_name".into(), table.clone().into());
        fields.remove("objects");
    }
    if let Some(alias) = fields.remove("alias") {
        attributes.insert("table_alias".into(), alias);
    }
    if !columns.is_empty() {
        attributes.insert("columns".into(), columns.into());
    }
}

fn expression_attributes(
    attributes: &mut Map<String, Value>,
    operator: &str,
    expressions: Vec<String>,
) {
    let mut plain = Vec::new();
    for expression in expressions {
        let labelled = LABELS.iter().find_map(|&(label, key, form)| {
            let rest = expression.strip_prefix(label)?.strip_prefix(':')?.trim();
            Some((key, form, rest))
        });
        let Some((key, form, rest)) = labelled else {
            plain.push(expression);
            continue;
        };
        let value = match form {
            Form::List => split_list(rest).into(),
            Form::Text => rest.into(),
            Form::Number => match rest.parse::<f64>() {
                Ok(number) => number.into(),
                Err(_) => rest.into(),
            },
        };
        match (attributes.get_mut(key), value) {
            (Some(Value::Array(existing)), Value::Array(more)) => existing.extend(more),
            (Some(Value::String(existing)), Value::String(more)) => {
                *existing = format!("{existing} AND {more}")
            }
            (_, value) => {
                attributes.insert(key.into(), value);
            }
        }
    }
    if plain.is_empty() {
        return;
    }
    let (key, value): (&str, Value) = match operator {
        "Filter" => ("filter_condition", plain.join(" AND ").into()),
        "WithClause" => ("name", plain.join(", ").into()),
        "WithReference" => ("alias", plain.join(", ").into()),
        "WindowFunction" => ("functions", plain.into()),
        _ => ("expressions", plain.into()),
    };
    attributes.insert(key.into(), value);
}

/// The `join_type` operator stats would give a join that `EXPLAIN` names
/// e.g. `LeftOuterJoin`: `LEFT OUTER`.
fn join_type(operator: &str) -> Option<String> {
    let kind = operator
        .strip_suffix("Join")
        .filter(|kind| !kind.is_empty())?;
    let mut words = String::new();
    for (index, c) in kind.char_indices() {
        if index > 0 && c.is_uppercase() {
            words.push(' ');
        }
        words.extend(c.to_uppercase());
    }
    Some(words)
}

/// Splits `[A, f(B, C), D]` (brackets optional) at its top-level commas.
fn split_list(text: &str) -> Vec<String> {
    let text = text
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(text);
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut quote = None;
    let mut start = 0;
    for (index, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') => depth -= 1,
            (None, ',') if depth == 0 => {
                items.push(text[start..index].trim().to_string());
                start = index + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        items.push(last.to_string());
    }
    items
}
