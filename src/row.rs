use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One row of `SELECT * FROM TABLE(GET_QUERY_OPERATOR_STATS(<query_id>))`.
///
/// Column names are accepted in Snowflake's uppercase form or in lowercase.
/// Drivers differ in how they return non-string columns: some return
/// `OPERATOR_STATISTICS` as an object and `STEP_ID` as a number, while
/// others (such as the SQL REST API) return them as JSON text. Both work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperatorStatsRow {
    #[serde(alias = "QUERY_ID")]
    pub query_id: String,

    #[serde(alias = "STEP_ID", deserialize_with = "json_or_text")]
    pub step_id: u32,

    #[serde(alias = "OPERATOR_ID", deserialize_with = "json_or_text")]
    pub operator_id: u32,

    /// `None` for the final operator, which is usually `Result`.
    #[serde(alias = "PARENT_OPERATORS", default, deserialize_with = "json_or_text")]
    pub parent_operators: Option<Vec<u32>>,

    #[serde(alias = "OPERATOR_TYPE")]
    pub operator_type: String,

    #[serde(
        alias = "OPERATOR_STATISTICS",
        default,
        deserialize_with = "json_or_text"
    )]
    pub operator_statistics: Value,

    #[serde(
        alias = "EXECUTION_TIME_BREAKDOWN",
        default,
        deserialize_with = "json_or_text"
    )]
    pub execution_time_breakdown: Value,

    #[serde(
        alias = "OPERATOR_ATTRIBUTES",
        default,
        deserialize_with = "json_or_text"
    )]
    pub operator_attributes: Value,
}

/// Parses a JSON array of [`OperatorStatsRow`]s.
pub fn rows_from_json(json: &str) -> Result<Vec<OperatorStatsRow>, serde_json::Error> {
    serde_json::from_str(json)
}

/// Reads a value that is either plain JSON or JSON written out as a string,
/// e.g. `{"input_rows": 5}` or `"{\"input_rows\": 5}"`, and `3` or `"3"`.
fn json_or_text<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = match Value::deserialize(deserializer)? {
        Value::String(text) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        value => value,
    };
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}
