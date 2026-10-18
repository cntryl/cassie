//! Existing-scalar row-constructor predicates under #800.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_resolve_null_equality_given_later_unequal_fields_when_rows_are_compared() {
    // Arrange
    let fixture = sql_fixture("row-null-equality", &[]);
    // Act
    let result = fixture.execute("SELECT (NULL, 2) = (NULL, 3), (NULL, 2) <> (NULL, 3), (1, NULL) = (1, NULL), (1, NULL) <> (1, NULL)");
    // Assert
    assert_eq!(
        result.expect("native row equality").rows(),
        &[vec![
            Value::Bool(false),
            Value::Bool(true),
            Value::Null,
            Value::Null
        ]]
    );
}

#[test]
fn should_preserve_exact_fields_given_bigint_and_text_when_tuple_ordering_executes() {
    // Arrange
    let fixture = sql_fixture("row-exact-ordering", &[]);
    // Act
    let result = fixture.execute("SELECT (9007199254740993::BIGINT, 'z') < (9007199254740994::BIGINT, 'a'), (1, 'a') <= (1, 'a'), (2, 'a') > (1, 'z'), (1, 'b') >= (1, 'a')");
    // Assert
    assert_eq!(
        result.expect("exact native row ordering").rows(),
        &[vec![Value::Bool(true); 4]]
    );
}

#[test]
fn should_parse_row_grouping_given_explicit_singletons_when_parentheses_are_nested() {
    // Arrange
    let fixture = sql_fixture("row-grouping", &[]);
    // Act
    let result =
        fixture.execute("SELECT ROW(1) < ROW(2), (1, 2) = ((1, 2)), ((ROW(1, 2))) <> ROW(1, 3)");
    // Assert
    assert_eq!(
        result.expect("row grouping").rows(),
        &[vec![Value::Bool(true); 3]]
    );
}

#[test]
fn should_compare_empty_rows_given_null_safe_operators_when_distinctness_is_requested() {
    // Arrange
    let fixture = sql_fixture("row-empty-distinct", &[]);
    // Act
    let result =
        fixture.execute("SELECT ROW() IS DISTINCT FROM ROW(), ROW() IS NOT DISTINCT FROM ROW()");
    // Assert
    assert_eq!(
        result.expect("native empty-row distinctness").rows(),
        &[vec![Value::Bool(false), Value::Bool(true)]]
    );
}

#[test]
fn should_filter_values_given_row_predicates_when_columns_are_bound() {
    // Arrange
    let fixture = sql_fixture("row-values-filter", &[]);
    // Act
    let result = fixture.execute(
        "SELECT a FROM (VALUES (1, 2), (1, 3), (2, NULL)) AS v(a, b) WHERE (a, b) < (1, 3)",
    );
    // Assert
    assert_eq!(
        result.expect("row source columns").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_boolean_descriptors_given_false_filters_when_rows_are_compared() {
    // Arrange
    let fixture = sql_fixture("row-empty-output", &[]);
    // Act
    let result = fixture
        .execute("SELECT (a, b) < (2, 0) AS p FROM (VALUES (1, NULL::INT)) AS v(a, b) WHERE FALSE");
    // Assert
    let result = result.expect("Boolean descriptor before empty output");
    assert_eq!(result.columns()[0].type_oid, 16);
    assert_eq!(result.rows(), Vec::<Vec<Value>>::new());
}

#[test]
fn should_stop_runtime_fields_given_decisive_ordering_when_later_division_would_fail() {
    // Arrange
    let fixture = sql_fixture(
        "row-runtime-short-circuit",
        &[
            "CREATE TABLE row_runtime(a INT, b INT)",
            "INSERT INTO row_runtime VALUES (1,0)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT (a, 1 / b) < (a + 1, 0) FROM row_runtime");
    // Assert
    assert_eq!(
        result.expect("later runtime field is not evaluated").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_dead_case_branches_given_row_constant_errors_when_expressions_are_planned() {
    // Arrange
    let fixture = sql_fixture("row-dead-case", &[]);
    // Act
    let result = fixture.execute("SELECT CASE WHEN TRUE THEN TRUE ELSE (1, 1 / 0) < (2, 0) END, CASE WHEN ROW((1,2)=(1,2))=ROW(TRUE) THEN TRUE ELSE (1/0)=0 END");
    // Assert
    assert_eq!(
        result.expect("dead row CASE branch").rows(),
        &[vec![Value::Bool(true); 2]]
    );
}

#[test]
fn should_prune_variable_fields_given_deciding_constants_when_row_equality_is_planned() {
    // Arrange
    let fixture = sql_fixture(
        "row-constant-pruning",
        &[
            "CREATE TABLE row_prune(a INT)",
            "INSERT INTO row_prune VALUES (0)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT (1 / a, 1) = (0, 2), (1 / a, 1) <> (0, 2), (1 / a, 1) IS DISTINCT FROM (0, 2), (1 / a, 1) IS NOT DISTINCT FROM (0, 2) FROM row_prune");
    // Assert
    assert_eq!(
        result.expect("native constant pruning").rows(),
        &[vec![
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false)
        ]]
    );
}

#[test]
fn should_preserve_text_identity_given_timestamp_spellings_when_row_fields_are_compared() {
    // Arrange
    let fixture = sql_fixture("row-text-identity", &[]);
    // Act
    let result = fixture.execute("SELECT ROW('2024-01-01T00:00:00Z') = ROW('2024-01-01T00:00:00.000Z'), ROW('2024-01-01T00:00:00Z') > ROW('2024-01-01T00:00:00.000Z'), (1, '2') = (1, 2), (1, 'abc') = (1, 'a'::VARCHAR(1))");
    // Assert
    assert_eq!(
        result
            .expect("native text identity and unknown coercion")
            .rows(),
        &[vec![
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false)
        ]]
    );
}

#[test]
fn should_prune_dead_variable_branches_given_conditional_constants_when_row_equality_is_planned() {
    // Arrange
    let fixture = sql_fixture(
        "row-conditional-pruning",
        &[
            "CREATE TABLE row_conditional(a INT)",
            "INSERT INTO row_conditional VALUES (0)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT ROW(1/a, CASE WHEN TRUE THEN 1 ELSE a END) = ROW(0,2), ROW(1/a, LOWER('a')) = ROW(0,'b'), ROW(1/a, COALESCE(1,a)) = ROW(0,2), ROW(1/a, (FALSE AND a=0)) = ROW(0,TRUE), ROW(1/a, NOT(FALSE AND a=0)) = ROW(0,FALSE), ROW(1/a, ((FALSE AND a=0) IS NULL)) = ROW(0,TRUE), ROW(1/a, (1 BETWEEN 2 AND a)) = ROW(0,TRUE), ROW(1/a, (1 NOT BETWEEN 2 AND a)) = ROW(0,FALSE), ROW(1/a, CAST(CASE WHEN TRUE THEN 1 ELSE a END AS BIGINT)) = ROW(0,2), ROW(1/a, (CASE WHEN TRUE THEN 1 ELSE a END)+1) = ROW(0,3), ROW(1/a, ((1,NULL) IS NOT DISTINCT FROM (1,NULL))) = ROW(0,FALSE), ROW(1/a, ABS(CASE WHEN TRUE THEN -1 ELSE a END)) = ROW(0,2), ROW(1/a, COALESCE(NULL, CASE WHEN TRUE THEN 1 ELSE a END, a)) = ROW(0,2) FROM row_conditional");
    // Assert
    assert_eq!(
        result.expect("native conditional pruning").rows(),
        &[vec![Value::Bool(false); 13]]
    );
}

#[test]
fn should_release_row_admission_given_a_memory_limit_when_follow_up_queries_execute() {
    // Arrange
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("config");
    config.limits.query_memory_budget_bytes = 128 * 1024;
    let fixture =
        crate::support_sql_fixture::sql_fixture_with_config("row-memory-admission", &[], config);
    let before = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].clone();
    let fields = vec!["CAST('x' AS TEXT)"; 1664].join(",");
    let sql = format!("SELECT ROW({fields}) = ROW({fields})");
    // Act
    let rejected = fixture.execute(&sql);
    let after = fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].clone();
    let recovery = fixture.execute("SELECT ROW(1) = ROW(1)");
    // Assert
    assert!(
        matches!(rejected, Err(cassie::app::CassieError::ResourceLimit(_))),
        "{rejected:?}"
    );
    assert_eq!(before, after);
    assert_eq!(
        recovery.expect("row owners released").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_recover_row_queries_given_prior_cancellation_when_the_same_instance_is_reused() {
    // Arrange
    let fixture = sql_fixture("row-cancel-recovery", &[]);
    let cancellation = cassie::runtime::QueryCancellationHandle::new();
    cancellation.cancel();
    // Act
    let rejected = fixture.cassie.execute_sql_with_cancellation(
        &fixture.session,
        "SELECT ROW(1,2) < ROW(2,0)",
        vec![],
        &cancellation,
    );
    let recovery = fixture.execute("SELECT ROW(1,2) < ROW(2,0)");
    // Assert
    assert!(
        matches!(rejected, Err(cassie::app::CassieError::QueryCancelled)),
        "{rejected:?}"
    );
    assert_eq!(
        recovery.expect("fresh cancellation owner").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_select_float_comparisons_given_explicit_casts_when_integer_fields_are_promoted() {
    // Arrange
    let fixture = sql_fixture("row-float-promotion", &[]);
    // Act
    let result = fixture.execute("SELECT ROW(9007199254740993::BIGINT) = ROW(9007199254740992::FLOAT8), ROW(0.1::FLOAT8 + 0.2::FLOAT8) = ROW(0.3::FLOAT8), ROW(1::SMALLINT) = ROW(1::BIGINT)");
    // Assert
    assert_eq!(
        result.expect("native floating operator selection").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_reuse_grouped_row_values_given_equivalent_function_names_when_predicates_are_projected() {
    // Arrange
    let fixture = sql_fixture(
        "row-grouped-projection",
        &[
            "CREATE TABLE row_group(a INT)",
            "INSERT INTO row_group VALUES (1),(1),(2)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT ROW(a, lower('A')) < ROW(2,'a') AS p, COUNT(*) FROM row_group GROUP BY ROW(a, LOWER('A')) < ROW(2,'a') ORDER BY p");
    // Assert
    assert_eq!(
        result.expect("grouped row identity").rows(),
        &[
            vec![Value::Bool(false), Value::Int64(1)],
            vec![Value::Bool(true), Value::Int64(2)]
        ]
    );
}
