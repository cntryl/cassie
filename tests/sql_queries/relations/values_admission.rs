//! VALUES source admission and existing primitive parameter identities.
//! Native character and arithmetic obligations remain tracked by #796/#800.
use crate::support_sql_fixture::{sql_fixture, sql_fixture_with_config};
use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

#[test]
fn should_preserve_parameter_identities_given_typed_values_columns_when_boolean_and_bigint_results_are_described(
) {
    // Arrange
    let fixture = sql_fixture("values-primitive-parameters", &[]);
    let cases = [
        (
            "SELECT x FROM (VALUES ($1), (9007199254740993::BIGINT)) AS v(x) ORDER BY x",
            vec![Value::Int64(7)],
            20,
            vec![
                vec![Value::Int64(7)],
                vec![Value::Int64(9_007_199_254_740_993)],
            ],
        ),
        (
            "SELECT BOOL_AND(flag) FROM (VALUES ($1), (NULL::BOOLEAN)) AS v(flag)",
            vec![Value::Bool(true)],
            16,
            vec![vec![Value::Bool(true)]],
        ),
        (
            "SELECT flag FROM (VALUES ($1)) AS v(flag) WHERE FALSE",
            vec![Value::Bool(true)],
            16,
            Vec::new(),
        ),
    ];
    // Act
    let outcomes = cases.map(|(sql, params, oid, rows)| {
        (
            fixture.cassie.execute_sql(&fixture.session, sql, params),
            oid,
            rows,
        )
    });
    // Assert
    for (outcome, oid, rows) in outcomes {
        let outcome = outcome.expect("typed primitive VALUES");
        assert_eq!(outcome.columns()[0].type_oid, oid);
        assert_eq!(outcome.rows(), rows);
    }
}

#[test]
fn should_release_values_owners_given_result_and_memory_limits_when_follow_up_queries_execute() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("config");
    config.limits.max_result_rows = 2;
    config.limits.query_memory_budget_bytes = 128 * 1024;
    let fixture = sql_fixture_with_config("values-resource-admission", &[], config);
    let before = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].clone();
    // Act
    let row_limit = fixture.execute("VALUES (1), (2), (3)");
    let admitted = fixture.execute("VALUES (1), (2), (3) LIMIT 1");
    let aggregate = fixture.execute("SELECT COUNT(*) FROM (VALUES (1), (2), (3)) AS input(n)");
    let memory_limit = fixture.execute("VALUES (CAST('x' AS CHAR(1000000)))");
    let array_expansion = fixture.execute("VALUES (CAST('{x}' AS CHAR(1000000)[]))");
    let after = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].clone();
    let recovery = fixture.execute("SELECT 45");
    // Assert
    assert!(matches!(row_limit, Err(CassieError::ResourceLimit(_))));
    assert_eq!(
        admitted.expect("bounded VALUES limit").rows(),
        &[vec![Value::Int64(1)]]
    );
    assert_eq!(
        aggregate
            .expect("result cap does not cap aggregate inputs")
            .rows(),
        &[vec![Value::Int64(3)]]
    );
    assert!(
        matches!(memory_limit, Err(CassieError::ResourceLimit(_))),
        "{memory_limit:?}"
    );
    assert!(
        matches!(array_expansion, Err(CassieError::Unsupported(_))),
        "{array_expansion:?}"
    );
    assert_eq!(before, after);
    assert_eq!(
        recovery.expect("follow up VALUES failure").rows(),
        &[vec![Value::Int64(45)]]
    );
}

#[test]
fn should_preserve_values_text_given_different_varchar_typmods_when_unknown_literals_are_coerced() {
    // Arrange
    let fixture = sql_fixture("values-varchar-typmods", &[]);
    let cases = [
        "VALUES ('abc'::VARCHAR(3)), ('abcdef')",
        "VALUES ('abc'::VARCHAR(3)), ('abcdef'::VARCHAR(6))",
    ];
    // Act
    let outcomes = cases.map(|sql| fixture.execute(sql));
    // Assert
    for outcome in outcomes {
        let outcome = outcome.expect("common unconstrained varchar");
        assert_eq!(outcome.columns()[0].type_oid, 1043);
        assert_eq!(
            outcome.rows(),
            &[
                vec![Value::String("abc".into())],
                vec![Value::String("abcdef".into())]
            ]
        );
    }
}

#[test]
fn should_preserve_values_character_identity_given_mixed_text_types_when_padding_is_coerced() {
    // Arrange
    let fixture = sql_fixture("values-character-padding", &[]);
    let cases = [
        (
            "VALUES ('a'::CHAR(3)), ('b'::VARCHAR(4))",
            1042,
            vec![
                vec![Value::String("a  ".into())],
                vec![Value::String("b".into())],
            ],
        ),
        (
            "VALUES ('a'::VARCHAR(4)), ('b'::CHAR(3))",
            1043,
            vec![
                vec![Value::String("a".into())],
                vec![Value::String("b".into())],
            ],
        ),
        (
            "VALUES ('a'::TEXT), ('b'::CHAR(3))",
            25,
            vec![
                vec![Value::String("a".into())],
                vec![Value::String("b".into())],
            ],
        ),
    ];
    // Act
    let outcomes = cases.map(|(sql, oid, rows)| (fixture.execute(sql), oid, rows));
    // Assert
    for (outcome, oid, rows) in outcomes {
        let outcome = outcome.expect("VALUES common text identity");
        assert_eq!(outcome.columns()[0].type_oid, oid);
        assert_eq!(outcome.rows(), rows);
    }
}

#[test]
fn should_validate_nested_ctes_given_a_values_body_when_a_declared_relation_is_missing() {
    // Arrange
    let fixture = sql_fixture("values-nested-cte", &[]);
    // Act
    let outcome = fixture
        .execute("SELECT * FROM (WITH unused AS (SELECT missing FROM absent) VALUES (1)) AS v");
    // Assert
    assert!(
        matches!(outcome, Err(CassieError::CatalogObjectNotFound { kind: cassie::app::CatalogObjectKind::Relation, ref name }) if name == "absent"),
        "{outcome:?}"
    );
}

#[test]
fn should_preserve_integer_operator_width_given_values_rows_when_addition_is_evaluated() {
    // Arrange
    let fixture = sql_fixture("values-int4-arithmetic", &[]);
    // Act
    let single_row = fixture.execute("VALUES (1 + 2)");
    let multi_column = fixture.execute("VALUES (1 + 2, 4)");
    let outcome = fixture.execute("VALUES (1 + 2), (4)");
    let derived = fixture.execute("SELECT n FROM (VALUES (1 + 2)) AS v(n)");
    let derived_multi_column = fixture.execute("SELECT a, b FROM (VALUES (1 + 2, 4)) AS v(a, b)");
    let cte = fixture.execute("WITH v(n) AS (VALUES (1 + 2)) SELECT n FROM v");
    let cte_multi_column =
        fixture.execute("WITH v(a, b) AS (VALUES (1 + 2, 4)) SELECT a, b FROM v");
    // Assert
    let single_row = single_row.expect("single-row INT4 arithmetic VALUES");
    assert_eq!(single_row.columns()[0].type_oid, 23);
    assert_eq!(single_row.rows(), &[vec![Value::Int64(3)]]);
    let multi_column = multi_column.expect("multi-column INT4 arithmetic VALUES");
    assert_eq!(
        multi_column
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        multi_column.rows(),
        &[vec![Value::Int64(3), Value::Int64(4)]]
    );
    let outcome = outcome.expect("INT4 arithmetic VALUES");
    assert_eq!(outcome.columns()[0].type_oid, 23);
    assert_eq!(
        outcome.rows(),
        &[vec![Value::Int64(3)], vec![Value::Int64(4)]]
    );
    let derived = derived.expect("derived INT4 arithmetic VALUES");
    assert_eq!(derived.columns()[0].type_oid, 23);
    assert_eq!(derived.rows(), &[vec![Value::Int64(3)]]);
    let derived_multi_column =
        derived_multi_column.expect("derived multi-column INT4 arithmetic VALUES");
    assert_eq!(
        derived_multi_column
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        derived_multi_column.rows(),
        &[vec![Value::Int64(3), Value::Int64(4)]]
    );
    let cte = cte.expect("CTE INT4 arithmetic VALUES");
    assert_eq!(cte.columns()[0].type_oid, 23);
    assert_eq!(cte.rows(), &[vec![Value::Int64(3)]]);
    let cte_multi_column = cte_multi_column.expect("CTE multi-column INT4 arithmetic VALUES");
    assert_eq!(
        cte_multi_column
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        cte_multi_column.rows(),
        &[vec![Value::Int64(3), Value::Int64(4)]]
    );
}
