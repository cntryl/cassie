//! Single-row VALUES propagation, scope and resource contracts under #800.
use crate::support_sql_fixture::{sql_fixture, sql_fixture_with_config};
use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

#[test]
fn should_prune_unused_constant_errors_given_single_values_rows_when_only_literals_are_selected() {
    // Arrange
    let fixture = sql_fixture("values-unused-expression", &[]);
    // Act
    let result = fixture.execute("SELECT 1 FROM (VALUES (1/0)) AS v(unused)");
    // Assert
    assert_eq!(
        result.expect("unused source expression is pruned").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_count_rows_given_unused_constant_errors_when_values_fields_are_not_read() {
    // Arrange
    let fixture = sql_fixture("values-unused-count", &[]);
    // Act
    let result = fixture.execute("SELECT COUNT(*) FROM (VALUES (1/0)) AS v(unused)");
    // Assert
    let result = result.expect("COUNT reads row presence");
    assert_eq!(result.columns()[0].type_oid, 20);
    assert_eq!(result.rows(), &[vec![Value::Int64(1)]]);
}

#[test]
fn should_preserve_values_descriptors_given_constant_propagation_when_aliases_are_projected() {
    // Arrange
    let fixture = sql_fixture("values-constant-types", &[]);
    // Act
    let result = fixture.execute("SELECT n, flag, label FROM (VALUES (9007199254740993::BIGINT, TRUE, NULL::TEXT)) AS v(n, flag, label)");
    // Assert
    let result = result.expect("typed propagated fields");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![("n", 20), ("flag", 16), ("label", 25)]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(9_007_199_254_740_993),
            Value::Bool(true),
            Value::Null
        ]]
    );
}

#[test]
fn should_preserve_delimited_aliases_given_unicode_names_when_constants_are_propagated() {
    // Arrange
    let fixture = sql_fixture("values-constant-unicode-alias", &[]);
    // Act
    let result = fixture.execute("SELECT \"Δ\" FROM (VALUES ('🙂'::TEXT)) AS \"Σ\"(\"Δ\")");
    // Assert
    let result = result.expect("delimited source alias");
    assert_eq!(result.columns()[0].name, "Δ");
    assert_eq!(result.columns()[0].type_oid, 25);
    assert_eq!(result.rows(), &[vec![Value::String("🙂".into())]]);
}

#[test]
fn should_preserve_repeated_fields_given_exact_bigints_when_source_references_are_replaced() {
    // Arrange
    let fixture = sql_fixture("values-repeated-constant", &[]);
    // Act
    let result = fixture.execute("SELECT n, n FROM (VALUES (9007199254740993::BIGINT)) AS v(n)");
    // Assert
    let result = result.expect("repeated BIGINT field");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 20]
    );
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(9_007_199_254_740_993); 2]]
    );
}

#[test]
fn should_keep_multirow_fields_variable_given_late_division_when_row_ordering_short_circuits() {
    // Arrange
    let fixture = sql_fixture("values-multirow-folding", &[]);
    // Act
    let result =
        fixture.execute("SELECT ROW(a,1/b) < ROW(a+1,0) FROM (VALUES (1,0),(2,0)) AS v(a,b)");
    // Assert
    assert_eq!(
        result.expect("multirow source remains variable").rows(),
        &[vec![Value::Bool(true)], vec![Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_empty_descriptors_given_false_filters_when_source_columns_are_inlined() {
    // Arrange
    let fixture = sql_fixture("values-constant-empty-types", &[]);
    // Act
    let result = fixture
        .execute("SELECT n, flag FROM (VALUES (1::SMALLINT, TRUE)) AS v(n, flag) WHERE FALSE");
    // Assert
    let result = result.expect("empty propagated output");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![("n", 21), ("flag", 16)]
    );
    assert_eq!(result.rows(), Vec::<Vec<Value>>::new());
}

#[test]
fn should_keep_parameter_sources_typed_given_explicit_casts_when_constant_eligibility_is_checked() {
    // Arrange
    let fixture = sql_fixture("values-constant-parameter-boundary", &[]);
    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT n, flag FROM (VALUES ($1::INT4, $2::BOOLEAN)) AS v(n,flag)",
        vec![Value::Int64(7), Value::Bool(true)],
    );
    // Assert
    let result = result.expect("source parameters remain live");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 16]
    );
    assert_eq!(result.rows(), &[vec![Value::Int64(7), Value::Bool(true)]]);
}

#[test]
fn should_preserve_correlated_fields_given_exists_consumers_when_inlining_is_ineligible() {
    // Arrange
    let fixture = sql_fixture(
        "values-constant-correlation",
        &[
            "CREATE TABLE const_lookup(k INT)",
            "INSERT INTO const_lookup VALUES(1)",
        ],
    );
    // Act
    let result = fixture
        .execute("SELECT EXISTS(SELECT 1 FROM const_lookup WHERE k=v.a) FROM (VALUES(1)) AS v(a)");
    // Assert
    assert_eq!(
        result.expect("correlated source field is retained").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_keep_nested_alias_scopes_distinct_given_shadowed_names_when_values_are_read() {
    // Arrange
    let fixture = sql_fixture("values-constant-shadowing", &[]);
    // Act
    let result = fixture.execute(
        "SELECT v.a, EXISTS(SELECT 1 FROM (VALUES(2)) AS v(a) WHERE a=2) FROM (VALUES(1)) AS v(a)",
    );
    // Assert
    assert_eq!(
        result
            .expect("inner source does not capture outer field")
            .rows(),
        &[vec![Value::Int64(1), Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_scalar_row_foldability_given_source_boolean_fields_when_case_branches_are_pruned(
) {
    // Arrange
    let fixture = sql_fixture("values-row-equality-folding", &[]);
    // Act
    let result = fixture.execute(
        "SELECT CASE WHEN p THEN TRUE ELSE (1/0)=0 END FROM (VALUES((1,2)=(1,2))) AS v(p)",
    );
    // Assert
    assert_eq!(
        result.expect("propagated row equality is constant").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_prune_logical_constants_given_arithmetic_errors_when_scalar_boolean_operators_execute() {
    // Arrange
    let fixture = sql_fixture("scalar-logical-constant-pruning", &[]);
    // Act
    let result = fixture.execute("SELECT FALSE AND 1/0=0, TRUE OR 1/0=0");
    // Assert
    assert_eq!(
        result.expect("native Boolean constant pruning").rows(),
        &[vec![Value::Bool(false), Value::Bool(true)]]
    );
}

#[test]
fn should_skip_variable_errors_given_deciding_boolean_constants_when_table_predicates_execute() {
    // Arrange
    let fixture = sql_fixture(
        "scalar-logical-variable-pruning",
        &[
            "CREATE TABLE logical_probe(n INT)",
            "INSERT INTO logical_probe VALUES(0)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT 1/n=0 AND FALSE, 1/n=0 OR TRUE FROM logical_probe");
    // Assert
    assert_eq!(
        result
            .expect("planner constants prune variable fields")
            .rows(),
        &[vec![Value::Bool(false), Value::Bool(true)]]
    );
}

#[test]
fn should_release_constant_expansion_given_a_small_budget_when_follow_up_queries_execute() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("config");
    config.limits.query_memory_budget_bytes = 512 * 1024;
    let fixture = sql_fixture_with_config("values-expansion-admission", &[], config);
    let before = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].clone();
    let fields = vec!["n"; 512].join(",");
    let text = "x".repeat(2048);
    let sql = format!("SELECT {fields} FROM (VALUES ('{text}'::TEXT)) AS v(n) WHERE FALSE");
    // Act
    let result = fixture.execute(&sql);
    let after = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].clone();
    let recovery = fixture.execute("SELECT n FROM (VALUES (1)) AS v(n)");
    // Assert
    assert!(
        matches!(result, Err(CassieError::ResourceLimit(_))),
        "{result:?}"
    );
    assert_eq!(before, after);
    assert_eq!(
        recovery.expect("expansion owners released").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_admit_nested_constant_clones_given_case_and_rows_when_source_fields_are_repeated() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("config");
    config.limits.query_memory_budget_bytes = 512 * 1024;
    let fixture = sql_fixture_with_config("values-nested-expansion-admission", &[], config);
    let fields = vec!["n"; 256].join(",");
    let text = "x".repeat(2048);
    let expressions = [
        format!("CASE WHEN TRUE THEN '{text}'::TEXT ELSE '{text}'::TEXT END"),
        format!("ROW('{text}'::TEXT, 1) = ROW('{text}'::TEXT, 1)"),
        format!("'{text}'::TEXT IN ('{text}'::TEXT, 'other'::TEXT)"),
    ];
    let before = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].clone();
    // Act
    let results = expressions.map(|expr| {
        fixture.execute(&format!(
            "SELECT {fields} FROM (VALUES ({expr})) AS v(n) WHERE FALSE"
        ))
    });
    let after = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].clone();
    let recovery = fixture.execute("SELECT n FROM (VALUES (7)) AS v(n)");
    // Assert
    for result in results {
        assert!(
            matches!(result, Err(CassieError::ResourceLimit(_))),
            "{result:?}"
        );
    }
    assert_eq!(before, after);
    assert_eq!(
        recovery.expect("nested clone owners released").rows(),
        &[vec![Value::Int64(7)]]
    );
}
