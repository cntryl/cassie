use cassie::app::Cassie;
use cassie::types::Value;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-poc-{label}-{}", Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

fn execute_poc_setup(cassie: &Cassie, session: &cassie::app::CassieSession) {
    for sql in [
        "CREATE TABLE poc_orders (tenant_id TEXT, status TEXT, created_at INT, title TEXT, total INT)",
        "INSERT INTO poc_orders (tenant_id, status, created_at, title, total) VALUES ('acme', 'open', 1, 'first order', 42), ('acme', 'open', 2, 'second order', 99), ('acme', 'closed', 3, 'closed order', 10), ('other', 'open', 4, 'other tenant', 7)",
        "CREATE INDEX poc_orders_lookup_idx ON poc_orders USING btree (tenant_id, status, created_at)",
    ] {
        cassie.execute_sql(session, sql, vec![]).unwrap();
    }
}

#[test]
fn should_document_runnable_poc_example_command() {
    // Arrange
    let readme = std::fs::read_to_string("README.md").expect("read root README");
    let docs = std::fs::read_to_string("docs/README.md").expect("read docs README");

    // Act
    let has_root_command = readme.contains("cargo run --locked --example poc_read_model");
    let has_doc_link = docs.contains("poc-quickstart.md");

    // Assert
    assert!(
        has_root_command,
        "README should name the POC example command"
    );
    assert!(has_doc_link, "docs README should link the POC quickstart");
}

#[test]
fn should_execute_embedded_read_model_poc_flow() {
    // Arrange
    with_fallback();
    let path = data_dir("embedded");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("poc", None);
    execute_poc_setup(&cassie, &session);
    let health = cassie.health();

    // Act
    let orders = cassie
        .execute_sql(
            &session,
            "SELECT title, total FROM poc_orders WHERE tenant_id = 'acme' AND status = 'open' ORDER BY created_at LIMIT 2",
            vec![],
        )
        .unwrap();
    let totals = cassie
        .execute_sql(
            &session,
            "SELECT status, COUNT(*) AS orders FROM poc_orders WHERE tenant_id = 'acme' GROUP BY status ORDER BY status",
            vec![],
        )
        .unwrap();
    let explain = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT title, total FROM poc_orders WHERE tenant_id = 'acme' AND status = 'open' ORDER BY created_at LIMIT 2",
            vec![],
        )
        .unwrap();

    // Assert
    assert_eq!(health["ready"].as_bool(), Some(true));
    assert_eq!(
        orders.rows,
        vec![
            vec![Value::String("first order".to_string()), Value::Int64(42)],
            vec![Value::String("second order".to_string()), Value::Int64(99)],
        ]
    );
    assert_eq!(
        totals.rows,
        vec![
            vec![Value::String("closed".to_string()), Value::Int64(1)],
            vec![Value::String("open".to_string()), Value::Int64(2)],
        ]
    );
    let Value::String(plan) = &explain.rows[0][0] else {
        panic!("expected textual plan");
    };
    assert!(plan.contains("index=poc_orders_lookup_idx"), "plan={plan}");

    let _ = std::fs::remove_dir_all(path);
}
