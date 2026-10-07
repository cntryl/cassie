use super::support_typed_join::JoinFixture;
use cassie::types::Value;

#[test]
fn should_preserve_explicit_schema_ownership_before_shared_relation_aliases() {
    // Arrange
    let setup = [
        "CREATE SCHEMA a",
        "CREATE SCHEMA b",
        "CREATE TABLE a.records (x BIGINT, y BIGINT)",
        "CREATE TABLE b.records (x BIGINT, y BIGINT)",
        "INSERT INTO a.records VALUES (1, 2)",
        "INSERT INTO b.records VALUES (2, 9)",
    ];
    let native = JoinFixture::new(true, &setup);
    let scalar = JoinFixture::new(false, &setup);
    let bounded = "SELECT a.records.x, b.records.y FROM a.records JOIN b.records ON b.records.x = a.records.y";
    let loaded = "SELECT a.records.x, b.records.y FROM a.records JOIN b.records ON b.records.x = a.records.y ORDER BY a.records.x";

    // Act
    let bounded_rows = native.execute(bounded);
    let bounded_strategy = native.cassie.metrics()["joins"]["last_strategy"].clone();
    let actual = native.execute(loaded);
    let expected = scalar.execute(loaded);

    // Assert
    assert_eq!(actual.columns, expected.columns);
    assert_eq!(actual.rows, expected.rows);
    assert_eq!(actual.rows, vec![vec![Value::Int64(1), Value::Int64(9)]]);
    assert_eq!(bounded_rows.rows, actual.rows);
    assert_eq!(bounded_strategy, "vectorized");
    assert_eq!(
        native.cassie.metrics()["joins"]["last_strategy"],
        "typed_hash"
    );
}

#[test]
fn should_match_existing_join_for_typed_duplicate_null_and_outer_payloads() {
    // Arrange
    let setup = [
        "CREATE TABLE left_records (n BIGINT, tag TEXT)",
        "CREATE TABLE right_records (n FLOAT, tag TEXT)",
        "INSERT INTO left_records VALUES (0, 'zero'), (1, 'one'), (1, 'again'), (NULL, 'null'), (2, 'unmatched'), (9007199254740993, 'exact')",
        "INSERT INTO right_records VALUES (-0.0, 'negative-zero'), (0.0, 'positive-zero'), (1.0, 'first'), (1.0, 'second'), (NULL, 'never'), (9007199254740992.0, 'rounded')",
    ];
    let native = JoinFixture::new(true, &setup);
    let scalar = JoinFixture::new(false, &setup);
    let queries = [
        "SELECT left_records.tag, right_records.tag FROM left_records JOIN right_records ON left_records.n = right_records.n ORDER BY left_records.tag, right_records.tag",
        "SELECT left_records.tag, right_records.tag FROM left_records LEFT JOIN right_records ON left_records.n = right_records.n ORDER BY left_records.tag, right_records.tag NULLS LAST",
    ];

    // Act
    let native_results = queries.map(|query| native.execute(query));
    let scalar_results = queries.map(|query| scalar.execute(query));

    // Assert
    for (typed, expected) in native_results.iter().zip(&scalar_results) {
        assert_eq!(typed.columns, expected.columns);
        assert_eq!(typed.rows, expected.rows);
    }
    assert_eq!(native_results[0].rows.len(), 6);
    assert_eq!(native_results[1].rows.len(), 9);
    assert!(native_results[1]
        .rows
        .contains(&vec![Value::String("exact".into()), Value::Null]));
    assert_eq!(
        native.cassie.metrics()["joins"]["last_strategy"],
        "typed_hash"
    );
    assert_eq!(
        native.cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_preserve_residual_and_empty_outer_fallback_shapes() {
    // Arrange
    let setup = [
        "CREATE TABLE left_records (n BIGINT, tag TEXT)",
        "CREATE TABLE right_records (n BIGINT, tag TEXT)",
        "INSERT INTO left_records VALUES (1, 'one'), (2, 'two'), (NULL, 'null')",
        "INSERT INTO right_records VALUES (1, 'early'), (1, 'late'), (NULL, 'never')",
        "CREATE TABLE empty_records (n BIGINT, tag TEXT)",
    ];
    let native = JoinFixture::new(true, &setup);
    let scalar = JoinFixture::new(false, &setup);
    let queries = [
        "SELECT left_records.tag, right_records.tag FROM left_records LEFT JOIN right_records ON left_records.n = right_records.n AND right_records.tag = 'late' ORDER BY left_records.tag",
        "SELECT left_records.tag, empty_records.tag FROM left_records LEFT JOIN empty_records ON left_records.n = empty_records.n ORDER BY left_records.tag",
        "SELECT left_records.tag, right_records.tag FROM left_records FULL JOIN right_records ON left_records.n = right_records.n ORDER BY left_records.tag NULLS LAST, right_records.tag NULLS LAST",
    ];

    // Act
    let native_results = queries.map(|query| native.execute(query));
    let scalar_results = queries.map(|query| scalar.execute(query));

    // Assert
    for (typed, expected) in native_results.iter().zip(&scalar_results) {
        assert_eq!(typed.columns, expected.columns);
        assert_eq!(typed.rows, expected.rows);
    }
    assert_eq!(native_results[0].rows.len(), 3);
    assert_eq!(native_results[1].rows.len(), 3);
    assert!(native_results[1]
        .rows
        .iter()
        .all(|row| row[1] == Value::Null));
}

#[test]
fn should_preserve_quoted_join_keys_and_boolean_null_multiplicity() {
    // Arrange
    let setup = [
        "CREATE TABLE left_records (\"Key\" BOOLEAN, tag TEXT)",
        "CREATE TABLE right_records (\"Key\" BOOLEAN, tag TEXT)",
        "INSERT INTO left_records VALUES (TRUE, 'one'), (FALSE, 'zero'), (NULL, 'null')",
        "INSERT INTO right_records VALUES (TRUE, 'first'), (TRUE, 'second'), (FALSE, 'zero'), (NULL, 'never')",
    ];
    let native = JoinFixture::new(true, &setup);
    let scalar = JoinFixture::new(false, &setup);
    let sql = "SELECT left_records.tag, right_records.tag FROM left_records LEFT JOIN right_records ON left_records.\"Key\" = right_records.\"Key\" ORDER BY left_records.tag, right_records.tag NULLS LAST";

    // Act
    let actual = native.execute(sql);
    let expected = scalar.execute(sql);

    // Assert
    assert_eq!(actual.columns, expected.columns);
    assert_eq!(actual.rows, expected.rows);
    assert_eq!(actual.rows.len(), 4);
    assert_eq!(
        native.cassie.metrics()["joins"]["last_strategy"],
        "typed_hash"
    );
}

#[test]
fn should_preserve_array_json_outer_payload_and_staged_join_visibility() {
    // Arrange
    let setup = [
        "CREATE TABLE left_records (n BIGINT, payload BIGINT[])",
        "CREATE TABLE right_records (n BIGINT, doc JSON)",
    ];
    let native = JoinFixture::new(true, &setup);
    let scalar = JoinFixture::new(false, &setup);
    for fixture in [&native, &scalar] {
        fixture.execute_with_params(
            "INSERT INTO left_records VALUES (1, $1), (2, NULL)",
            vec![Value::Json(serde_json::json!([
                9_007_199_254_740_993_i64,
                null
            ]))],
        );
        fixture.execute("BEGIN");
        fixture.execute_with_params(
            "INSERT INTO right_records VALUES (1, $1), (1, $2)",
            vec![
                Value::Json(serde_json::Value::Null),
                Value::Json(serde_json::json!({"staged": true})),
            ],
        );
    }
    let sql = "SELECT left_records.n, left_records.payload, right_records.doc FROM left_records LEFT JOIN right_records ON left_records.n = right_records.n ORDER BY left_records.n, right_records.doc";

    // Act
    let actual = native.execute(sql);
    let expected = scalar.execute(sql);

    // Assert
    assert_eq!(actual.columns, expected.columns);
    assert_eq!(actual.rows, expected.rows);
    assert_eq!(actual.rows.len(), 3);
    assert!(actual
        .rows
        .iter()
        .any(|row| row[2] == Value::Json(serde_json::Value::Null)));
    assert_eq!(
        actual.rows.last().unwrap(),
        &vec![Value::Int64(2), Value::Null, Value::Null]
    );
    assert_eq!(
        native.cassie.metrics()["joins"]["last_strategy"],
        "typed_hash"
    );
    native.execute("ROLLBACK");
    scalar.execute("ROLLBACK");
}
