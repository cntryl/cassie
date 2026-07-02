use cassie::types::Value;
use cassie::Cassie;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let data_dir = poc_data_dir();
    let cassie = Cassie::new_with_data_dir(&data_dir)?;
    cassie.startup()?;
    let session = cassie.create_session("poc", None);

    for sql in [
        "CREATE TABLE poc_orders (tenant_id TEXT, status TEXT, created_at INT, title TEXT, total INT)",
        "INSERT INTO poc_orders (tenant_id, status, created_at, title, total) VALUES ('acme', 'open', 1, 'first order', 42), ('acme', 'open', 2, 'second order', 99), ('acme', 'closed', 3, 'closed order', 10), ('other', 'open', 4, 'other tenant', 7)",
        "CREATE INDEX poc_orders_lookup_idx ON poc_orders USING btree (tenant_id, status, created_at)",
    ] {
        cassie.execute_sql(&session, sql, vec![])?;
    }

    let orders = cassie.execute_sql(
        &session,
        "SELECT title, total FROM poc_orders WHERE tenant_id = 'acme' AND status = 'open' ORDER BY created_at LIMIT 2",
        vec![],
    )?;
    let totals = cassie.execute_sql(
        &session,
        "SELECT status, COUNT(*) AS orders FROM poc_orders WHERE tenant_id = 'acme' GROUP BY status ORDER BY status",
        vec![],
    )?;
    let explain = cassie.execute_sql(
        &session,
        "EXPLAIN SELECT title, total FROM poc_orders WHERE tenant_id = 'acme' AND status = 'open' ORDER BY created_at LIMIT 2",
        vec![],
    )?;

    println!("Cassie POC read model");
    println!("data_dir={data_dir}");
    println!("health.ready={}", cassie.health()["ready"]);
    println!("open_orders={}", render_rows(&orders.rows));
    println!("tenant_totals={}", render_rows(&totals.rows));
    println!("plan={}", render_value(&explain.rows[0][0]));
    println!(
        "queries_recorded={}",
        cassie.metrics()["query"]["count"]
            .as_u64()
            .unwrap_or_default()
    );

    cassie.shutdown();
    let _ = std::fs::remove_dir_all(data_dir);
    Ok(())
}

fn poc_data_dir() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-poc-read-model-{}-{millis}",
        std::process::id()
    ));
    path.to_string_lossy().to_string()
}

fn render_rows(rows: &[Vec<Value>]) -> String {
    rows.iter()
        .map(|row| row.iter().map(render_value).collect::<Vec<_>>().join(","))
        .collect::<Vec<_>>()
        .join(";")
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Int64(value) => value.to_string(),
        Value::Float64(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Vector(value) => format!("{value:?}"),
        Value::Json(value) => value.to_string(),
    }
}
