use cassie::types::Value;

pub fn column_aggregate_unexpected_types(row: &[Value]) -> String {
    let types = row.iter().map(value_kind).collect::<Vec<_>>().join(", ");
    format!("Tier 3 column aggregate result has unexpected types: [{types}]")
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
