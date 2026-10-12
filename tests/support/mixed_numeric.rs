//! Shared SQL and literal PostgreSQL Boolean rows for mixed operator tests.
use cassie::types::Value;

pub fn projection(left: &str, right: &str) -> String {
    [
        ("=", "eq"),
        ("<>", "ne"),
        ("<", "lt"),
        ("<=", "le"),
        (">", "gt"),
        (">=", "ge"),
        ("IS NOT DISTINCT FROM", "same"),
        ("IS DISTINCT FROM", "different"),
    ]
    .map(|(operator, alias)| format!("{left} {operator} {right} AS {alias}"))
    .join(", ")
}

pub fn boolean_row(values: [Option<bool>; 8]) -> Vec<Value> {
    values
        .into_iter()
        .map(|value| value.map_or(Value::Null, Value::Bool))
        .collect()
}
