use super::*;

#[test]
fn should_retain_typed_scan_projection_for_ordinary_and_prefix_collection_aliases() {
    // Arrange
    let _guard = crate::midge::adapter::query_scan_control_test_guard();
    let path = std::env::temp_dir().join(format!("cassie-typed-alias-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE records (id INT, n BIGINT)",
        "INSERT INTO records VALUES (1,10),(2,20)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let queries = [
        "SELECT n FROM records WHERE n>=10 LIMIT 1",
        "SELECT r.n FROM records r WHERE r.n>=10 LIMIT 1",
        "SELECT r.amount FROM records r(key,amount) WHERE r.amount>=10 LIMIT 1",
    ];
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());

    // Act
    let results = queries.map(|sql| {
        let parsed = crate::sql::parse_statement(sql).expect("parse");
        let plan =
            super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &parsed)
                .expect("plan");
        try_execute(
            &cassie,
            Some(&session),
            &plan,
            &HashMap::new(),
            &[],
            &controls,
        )
    });

    // Assert
    for (sql, result) in queries.iter().zip(results) {
        let rows = result.expect(sql).expect("existing typed capability").0;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].entries()[0].1, Value::Int64(10));
        assert_eq!(rows[0].data_types(), &[crate::types::DataType::BigInt]);
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop((session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
}
