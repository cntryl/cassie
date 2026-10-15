#[test]
fn should_preserve_all_null_aggregate_semantics() {
    // Arrange
    let fixture = amount_fixture(
        "column_batch_all_null",
        "column_batch_all_null",
        &[Value::Null, Value::Null],
    );

    // Act
    let result = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT COUNT(*) AS rows, COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM column_batch_all_null",
            vec![],
        )
        .expect("aggregate null rows");

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![
            Value::Int64(2),
            Value::Int64(0),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ]]
    );
    assert_eq!(
        fixture.cassie.metrics()["aggregate_acceleration"]["scans"],
        1
    );

    let _ = std::fs::remove_dir_all(&fixture.path);
}
