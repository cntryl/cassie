#[test]
fn should_ignore_null_values_for_basic_aggregates_query() {
    // Arrange
    use_local_storage();
    let path = data_dir("aggregate_nulls");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE aggregate_null_sales (amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_null_sales (amount) VALUES (7)",
            "INSERT INTO aggregate_null_sales (amount) VALUES (NULL)",
            "INSERT INTO aggregate_null_sales (amount) VALUES (3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_null_sales",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![
                Value::Int64(2),
                Value::Int64(10),
                // avg(integer) is numeric in PostgreSQL 18.6.
                Value::String("5.0000000000000000".to_owned()),
                Value::Int64(3),
                Value::Int64(7)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_null_for_empty_or_all_null_boolean_aggregate_inputs() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "aggregate_boolean_empty_null",
        &[
            "CREATE TABLE aggregate_boolean_inputs (flag BOOLEAN)",
            "INSERT INTO aggregate_boolean_inputs VALUES (NULL), (NULL)",
        ],
    );

    // Act
    let empty = fixture.execute(
            "SELECT BOOL_AND(flag), BOOL_OR(flag), COUNT(flag) FROM aggregate_boolean_inputs WHERE false",
        );
    let all_null = fixture
        .execute("SELECT BOOL_AND(flag), BOOL_OR(flag), COUNT(flag) FROM aggregate_boolean_inputs");

    // Assert
    assert_eq!(
        empty
            .expect("boolean aggregates over an empty input")
            .rows(),
        &[vec![Value::Null, Value::Null, Value::Int64(0)]]
    );
    assert_eq!(
        all_null
            .expect("boolean aggregates over all-NULL inputs")
            .rows(),
        &[vec![Value::Null, Value::Null, Value::Int64(0)]]
    );
}
