use super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_emit_canonical_values_from_typed_casts() {
    // Arrange
    let fixture = sql_fixture("canonical_typed_casts", &[
            "CREATE TABLE typed_casts (id INT, u UUID, b BYTEA)",
            "INSERT INTO typed_casts VALUES (1, 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11', '\\xdeadbeef')",
        ]);

    // Act
    let rows = fixture.rows(
        "SELECT CAST('A0EEBC999C0B4EF8BB6D6BB9BD380A11' AS UUID), CAST('\\xDEADBEEF' AS BYTEA)",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".into()),
            Value::String("\\xdeadbeef".into())
        ]]
    );
    for predicate in [
        "u = CAST('A0EEBC999C0B4EF8BB6D6BB9BD380A11' AS UUID)",
        "b = CAST('\\xDEADBEEF' AS BYTEA)",
    ] {
        assert_eq!(
            fixture.rows(&format!("SELECT id FROM typed_casts WHERE {predicate}")),
            vec![vec![Value::Int64(1)]]
        );
    }
}

#[test]
fn should_apply_canonical_typed_literals_to_mutation_predicates() {
    // Arrange
    let fixture = sql_fixture(
        "canonical_typed_dml",
        &["CREATE TABLE typed_dml (id INT, u UUID, b BYTEA)"],
    );
    for predicate in [
        "u = 'A0EEBC999C0B4EF8BB6D6BB9BD380A11'",
        "u IN ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11')",
        "u BETWEEN 'A0EEBC999C0B4EF8BB6D6BB9BD380A11' AND 'A0EEBC999C0B4EF8BB6D6BB9BD380A11'",
        "b = '\\xDEADBEEF'",
        "b IN ('\\xDEADBEEF')",
        "b BETWEEN '\\xDEADBEEF' AND '\\xDEADBEEF'",
    ] {
        fixture.execute("INSERT INTO typed_dml VALUES (1, 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11', '\\xdeadbeef')").expect("seed");
        assert_eq!(
            fixture.rows(&format!("SELECT id FROM typed_dml WHERE {predicate}")),
            vec![vec![Value::Int64(1)]]
        );

        // Act
        fixture
            .execute(&format!("UPDATE typed_dml SET id = 2 WHERE {predicate}"))
            .expect("update");

        // Assert
        assert_eq!(
            fixture.rows("SELECT id FROM typed_dml"),
            vec![vec![Value::Int64(2)]]
        );
        fixture
            .execute(&format!("DELETE FROM typed_dml WHERE {predicate}"))
            .expect("delete");
        assert_eq!(
            fixture.rows("SELECT id FROM typed_dml"),
            [] as [Vec<Value>; 0]
        );
    }
}
#[test]
fn should_reject_incompatible_coalesce_types_before_execution() {
    // Arrange
    let fixture = sql_fixture(
        "coalesce_result_types",
        &[
            "CREATE TABLE coalesce_types (id INT, flag BOOLEAN, name TEXT, score INT)",
            "CREATE TABLE coalesce_other (id INT)",
            "INSERT INTO coalesce_other VALUES (2)",
            "INSERT INTO coalesce_types VALUES (1, true, 'x', 5), (2, NULL, 'y', 0)",
            "CREATE FUNCTION bool_identity(x BOOLEAN) RETURNS BOOLEAN AS \"x\"",
        ],
    );

    // Act
    for expression in [
        "COALESCE(flag, name)",
        "COALESCE(coalesce_types.flag, coalesce_types.name)",
        "COALESCE(flag, score)",
        "COALESCE(flag, ABS(score))",
        "COALESCE(bool_identity(flag), name)",
        "CASE WHEN id = 1 THEN true ELSE COALESCE(flag, name) END",
        "COALESCE(CASE WHEN id = 1 THEN flag ELSE NULL END, name)",
    ] {
        let result = fixture.execute(&format!(
            "SELECT {expression} FROM coalesce_types ORDER BY id"
        ));

        // Assert
        assert!(
            result.is_err(),
            "incompatible result accepted: {expression}"
        );
    }
    assert!(fixture.execute("SELECT COALESCE(coalesce_types.flag, coalesce_types.name) FROM coalesce_types JOIN coalesce_other ON coalesce_types.id = coalesce_other.id").is_err(), "qualified join COALESCE must reject incompatible types");
    for sql in [
        "UPDATE coalesce_types SET score = 99 RETURNING COALESCE(flag, name)",
        "DELETE FROM coalesce_types RETURNING COALESCE(flag, score)",
        "INSERT INTO coalesce_types VALUES (3, true, 'z', 7) RETURNING COALESCE(flag, name)",
    ] {
        assert!(
            fixture.execute(sql).is_err(),
            "incompatible RETURNING accepted: {sql}"
        );
    }
    assert_eq!(
        fixture.rows("SELECT COALESCE(flag, false) FROM coalesce_types ORDER BY id"),
        vec![vec![Value::Bool(true)], vec![Value::Bool(false)]]
    );
    assert_eq!(
        fixture.rows("SELECT COALESCE(NULL, score, 1.5) FROM coalesce_types ORDER BY id"),
        // COALESCE over integer and the numeric literal 1.5 is numeric.
        vec![
            vec![Value::String("5".to_owned())],
            vec![Value::String("0".to_owned())]
        ]
    );
}
#[test]
fn should_validate_bound_parameter_coalesce_result_types() {
    // Arrange
    let fixture = sql_fixture(
        "coalesce_parameter_types",
        &[
            "CREATE TABLE parameter_coalesce (flag BOOLEAN)",
            "CREATE TABLE uuid_parameter_coalesce (value UUID)",
            "CREATE TABLE array_parameter_coalesce (value INT[])",
            "INSERT INTO array_parameter_coalesce VALUES (NULL)",
            "INSERT INTO uuid_parameter_coalesce VALUES (NULL)",
            "INSERT INTO parameter_coalesce VALUES (NULL)",
        ],
    );

    // Act
    for query in [
            "SELECT COALESCE(flag, $1) FROM parameter_coalesce",
            "SELECT value FROM (SELECT COALESCE(flag, $1) AS value FROM parameter_coalesce) AS nested",
            "WITH values_cte AS (SELECT COALESCE(flag, $1) AS value FROM parameter_coalesce) SELECT value FROM values_cte",
            "WITH seed AS (SELECT flag FROM parameter_coalesce), values_cte AS (SELECT COALESCE(flag, $1) AS value FROM seed) SELECT value FROM values_cte",
            "SELECT flag FROM parameter_coalesce UNION ALL SELECT COALESCE(flag, $1) FROM parameter_coalesce",
            "UPDATE parameter_coalesce SET flag = true RETURNING COALESCE(flag, $1)",
        ] {
            let incompatible = fixture.cassie.execute_sql(&fixture.session, query, vec![Value::String("text".into())]);

            // Assert
            assert!(incompatible.is_err(), "boolean/text parameters accepted: {query}");
        }
    let compatible = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT COALESCE(flag, $1) FROM parameter_coalesce",
            vec![Value::Bool(false)],
        )
        .expect("compatible parameter");
    assert_eq!(compatible.rows(), vec![vec![Value::Bool(false)]]);
    let chained = fixture.cassie.execute_sql(&fixture.session,
            "WITH seed AS (SELECT flag FROM parameter_coalesce), values_cte AS (SELECT COALESCE(flag, $1) AS value FROM seed) SELECT value FROM values_cte",
            vec![Value::Bool(false)]).expect("compatible chained CTE");
    assert_eq!(chained.rows(), vec![vec![Value::Bool(false)]]);
    let canonical = "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11";
    let uuid_result = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT COALESCE(value, $1) FROM uuid_parameter_coalesce",
            vec![Value::String(canonical.into())],
        )
        .expect("UUID string parameter");
    assert_eq!(
        uuid_result.rows(),
        vec![vec![Value::String(canonical.into())]]
    );
    let array = serde_json::json!([1, 2]);
    let array_result = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT COALESCE(value, $1) FROM array_parameter_coalesce",
            vec![Value::Json(array.clone())],
        )
        .expect("array parameter");
    assert_eq!(array_result.rows(), vec![vec![Value::Json(array)]]);
}
