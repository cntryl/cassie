use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

pub fn seed_numeric_arrays(cassie: &Cassie, session: &CassieSession) {
    for statement in [
        "CREATE TABLE array_values (item TEXT, arr INT[])",
        "CREATE TABLE json_values (item TEXT, arr JSON)",
    ] {
        cassie
            .execute_sql(session, statement, vec![])
            .expect(statement);
    }
    for (item, array) in [
        ("a", serde_json::json!([2])),
        ("b", serde_json::json!([10])),
        ("c", serde_json::json!([])),
        ("d", serde_json::json!([1, 9])),
    ] {
        for table in ["array_values", "json_values"] {
            cassie
                .execute_sql(
                    session,
                    &format!("INSERT INTO {table} (item, arr) VALUES ($1, $2)"),
                    vec![Value::String(item.into()), Value::Json(array.clone())],
                )
                .expect("insert ARRAY/JSON ordering fixture");
        }
    }
}
