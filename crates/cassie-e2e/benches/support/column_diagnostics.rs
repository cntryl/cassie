use cassie::types::Value;

pub fn column_aggregate_unexpected_types(row: &[Value]) -> String {
    let types = row.iter().map(value_kind).collect::<Vec<_>>().join(", ");
    format!("Tier 3 column aggregate result has unexpected types: [{types}]")
}

pub struct ColumnAggregateResult<'a> {
    pub row_count: &'a i64,
    pub score_sum: &'a i64,
    pub score_average: &'a String,
    pub min_score: &'a i64,
    pub max_score: &'a i64,
}

pub fn column_aggregate_result_or_panic(row: &[Value]) -> ColumnAggregateResult<'_> {
    let [Value::Int64(row_count), Value::Int64(score_sum), Value::String(score_average), Value::Int64(min_score), Value::Int64(max_score)] =
        row
    else {
        panic!("{}", column_aggregate_unexpected_types(row));
    };

    ColumnAggregateResult {
        row_count,
        score_sum,
        score_average,
        min_score,
        max_score,
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int64(_) => "int64",
        Value::Float64(_) => "float64",
        Value::String(_) => "string",
        Value::Vector(_) => "vector",
        Value::Array(_) => "array",
        Value::Json(_) => "json",
        Value::Typed(_) => "typed",
    }
}
