#[test]
fn should_preserve_vector_extrema_given_empty_and_filtered_inputs_when_other_overloads_are_rejected(
) {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "aggregate_vector_extension_admission",
        &["CREATE TABLE aggregate_extension (v VECTOR(2), body JSON)"],
    );
    let invalid = [
        ("SUM(v)", "function sum(vector(2)) does not exist"),
        ("AVG(v)", "function avg(vector(2)) does not exist"),
        ("BOOL_AND(v)", "function bool_and(vector(2)) does not exist"),
        ("MIN(body)", "function min(json) does not exist"),
        ("MAX(body)", "function max(json) does not exist"),
        ("SUM(body)", "function sum(json) does not exist"),
        ("AVG(body)", "function avg(json) does not exist"),
    ];

    // Act
    let empty = fixture.execute("SELECT MIN(v),MAX(v) FROM aggregate_extension");
    let errors = invalid.map(|(aggregate, expected)| {
        let sql = format!("SELECT {aggregate} FILTER (WHERE FALSE) FROM aggregate_extension");
        (fixture.error(&sql).to_string(), expected)
    });
    for components in [vec![1.0, 2.0], vec![0.0, 10.0]] {
        fixture
            .cassie
            .execute_sql(
                &fixture.session,
                "INSERT INTO aggregate_extension (v) VALUES ($1)",
                vec![Value::Vector(Vector::new(components))],
            )
            .expect("seed vector extrema");
    }
    let populated = fixture.execute("SELECT MIN(v),MAX(v) FROM aggregate_extension");
    let filtered = fixture.execute(
        "SELECT MIN(v) FILTER (WHERE FALSE),MAX(v) FILTER (WHERE FALSE) FROM aggregate_extension",
    );

    // Assert
    let empty = empty.expect("empty vector extrema");
    assert_eq!(empty.rows(), &[vec![Value::Null, Value::Null]]);
    assert_eq!(
        empty
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![DataType::Vector(2).type_oid(); 2]
    );
    for (actual, expected) in errors {
        assert_eq!(actual, format!("execution error: {expected}"));
    }
    assert_eq!(
        populated.expect("populated vector extrema").rows(),
        &[vec![
            Value::Vector(Vector::new(vec![0.0, 10.0])),
            Value::Vector(Vector::new(vec![1.0, 2.0])),
        ]]
    );
    assert_eq!(
        filtered.expect("filtered vector extrema").rows(),
        &[vec![Value::Null, Value::Null]]
    );
}

#[test]
fn should_reject_float_sum_that_overflows_to_infinity() {
    // Arrange
    let fixture = float_overflow_fixture("aggregate_float_sum_overflow");

    // Act
    let error = fixture.error("SELECT SUM(v) FROM fl");

    // Assert
    assert!(
        matches!(&error, cassie::app::CassieError::Execution(message) if message == "value out of range: overflow"),
        "float overflow must be an execution error"
    );
}

#[test]
fn should_reject_float_avg_that_overflows_to_infinity() {
    // Arrange
    let fixture = float_overflow_fixture("aggregate_float_avg_overflow");

    // Act
    let error = fixture.error("SELECT AVG(v) FROM fl");

    // Assert
    assert!(
        matches!(&error, cassie::app::CassieError::Execution(message) if message == "value out of range: overflow"),
        "float overflow must be an execution error"
    );
}

#[test]
fn should_reject_filtered_column_batch_float_sum_overflow() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "aggregate_float_direct_overflow",
        &[
            "CREATE TABLE fd (id INT, v FLOAT)",
            "INSERT INTO fd (id, v) VALUES (1, 1.0e308)",
            "INSERT INTO fd (id, v) VALUES (2, 1.0e308)",
            "CREATE INDEX fd_idx ON fd USING column (id, v) WITH (segment_size = 1)",
        ],
    );

    // Act
    let errors = [
        "SELECT SUM(v) FROM fd WHERE id > 0",
        "SELECT AVG(v) FROM fd WHERE id > 0",
        "SELECT SUM(v) FROM fd",
        "SELECT AVG(v) FROM fd",
    ]
    .map(|sql| fixture.error(sql).to_string());

    // Assert
    for error in errors {
        assert!(
            error.contains("value out of range: overflow"),
            "float overflow must be reported"
        );
    }
}

#[test]
fn should_reject_numeric_aggregates_over_non_numeric_columns() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "aggregate_non_numeric_input",
        &[
            "CREATE TABLE agg (label TEXT, flag BOOLEAN)",
            "INSERT INTO agg (label, flag) VALUES ('a', true)",
            "INSERT INTO agg (label, flag) VALUES ('b', false)",
        ],
    );

    // Act
    let errors = [
        "SELECT SUM(label) FROM agg",
        "SELECT AVG(label) FROM agg",
        "SELECT SUM(flag) FROM agg",
        "SELECT AVG(flag) FROM agg",
        "SELECT SUM(label) AS s, COUNT(label) AS c FROM agg GROUP BY flag",
    ]
    .map(|sql| fixture.error(sql).to_string());

    // Assert
    assert!(
        errors
            == [
                "execution error: function sum(text) does not exist",
                "execution error: function avg(text) does not exist",
                "execution error: function sum(boolean) does not exist",
                "execution error: function avg(boolean) does not exist",
                "execution error: function sum(text) does not exist",
            ],
        "each aggregate must name its non-numeric input type"
    );
}

#[test]
fn should_reject_sum_over_mixed_json_values() {
    // Arrange
    let fixture = super::support_sql_fixture::sql_fixture(
        "aggregate_mixed_input",
        &["CREATE TABLE mixed (id INT, amount JSON)"],
    );
    let collection = super::support_sql::canonical_test_collection(&fixture.cassie, "mixed");
    fixture
        .cassie
        .midge
        .put_documents(
            &collection,
            vec![
                (
                    Some("m1".to_string()),
                    serde_json::json!({ "id": 1, "amount": 2 }),
                ),
                (
                    Some("m2".to_string()),
                    serde_json::json!({ "id": 2, "amount": "not a number" }),
                ),
                (
                    Some("m3".to_string()),
                    serde_json::json!({ "id": 3, "amount": 3 }),
                ),
            ],
        )
        .expect("put mixed documents");

    // Act
    let errors = [
        "SELECT SUM(amount) FROM mixed",
        "SELECT AVG(amount) FROM mixed",
    ]
    .map(|sql| fixture.error(sql).to_string());

    // Assert
    assert_eq!(
        errors,
        [
            "execution error: function sum(json) does not exist",
            "execution error: function avg(json) does not exist",
        ],
        "a non-numeric row in a mixed column must fail the aggregate"
    );
}

#[test]
fn should_reject_bare_column_beside_aggregate_without_group_by() {
    // Arrange
    let fixture = grouping_fixture("aggregate_implicit_group_bare_column");

    // Act
    let errors = [
        "SELECT g, SUM(v) AS s FROM m",
        "SELECT g, COUNT(*) AS n FROM m",
        "SELECT v, MAX(v) AS mx FROM m",
        "SELECT g FROM m HAVING COUNT(*) > 0",
        "SELECT v + 1, COUNT(*) FROM m",
    ]
    .map(|sql| fixture.error(sql).to_string());

    // Assert
    for error in errors {
        assert!(
            error
                .contains("must appear in the GROUP BY clause or be used in an aggregate function"),
            "an ungrouped column beside an aggregate must be rejected"
        );
    }
}

#[test]
fn should_reject_order_by_column_that_is_not_grouped() {
    // Arrange
    let fixture = grouping_fixture("aggregate_order_by_ungrouped");

    // Act
    let errors = [
        "SELECT g, COUNT(*) AS n FROM m GROUP BY g ORDER BY v DESC",
        "SELECT g, MAX(v) AS mx FROM m GROUP BY g ORDER BY v",
        "SELECT COUNT(*) FROM m ORDER BY v",
    ]
    .map(|sql| fixture.error(sql).to_string());

    // Assert
    for error in errors {
        assert!(
            error
                .contains("must appear in the GROUP BY clause or be used in an aggregate function"),
            "an ungrouped ORDER BY column must be rejected"
        );
    }
}
