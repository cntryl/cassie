use super::support_sql_fixture::sql_fixture;
use cassie::types::{DataType, Value};

#[test]
fn should_return_nullif_values_using_sql_equality() {
    // Arrange
    let fixture = sql_fixture("conditional_nullif", &[]);

    // Act
    let result =
        fixture.execute("SELECT NULLIF(4, 4), NULLIF(4, 5), NULLIF(4, NULL), NULLIF(NULL, 4)");

    // Assert
    assert_eq!(
        result.expect("NULLIF is supported").rows,
        vec![vec![
            Value::Null,
            Value::Int64(4),
            Value::Int64(4),
            Value::Null
        ]]
    );
}

#[test]
fn should_ignore_null_arguments_in_conditional_extrema() {
    // Arrange
    let fixture = sql_fixture("conditional_extrema", &[]);

    // Act
    let result = fixture.execute(
        "SELECT GREATEST(NULL, 4, 9), LEAST(4, NULL, 9), GREATEST(NULL, NULL), LEAST(NULL, NULL)",
    );

    // Assert
    assert_eq!(
        result.expect("conditional extrema are supported").rows,
        vec![vec![
            Value::Int64(9),
            Value::Int64(4),
            Value::Null,
            Value::Null
        ]]
    );
}

#[test]
fn should_compare_conditional_integer_values_without_float_rounding() {
    // Arrange
    let fixture = sql_fixture("conditional_exact_integer", &[]);

    // Act
    let result = fixture.execute("SELECT NULLIF(9007199254740993, 9007199254740992), GREATEST(9007199254740992, 9007199254740993), LEAST(9007199254740993, 9007199254740992)");

    // Assert
    assert_eq!(
        result.expect("integer comparisons remain exact").rows,
        vec![vec![
            Value::Int64(9_007_199_254_740_993),
            Value::Int64(9_007_199_254_740_993),
            Value::Int64(9_007_199_254_740_992),
        ]]
    );
}

#[test]
fn should_apply_conditional_expressions_inside_relational_arguments() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_expression_boundaries",
        &[
            "CREATE TABLE records (n INT)",
            "INSERT INTO records VALUES (0), (2), (4)",
        ],
    );

    // Act
    let aggregate = fixture.execute("SELECT SUM(NULLIF(n, 0)) FROM records");
    let window = fixture
        .execute("SELECT FIRST_VALUE(GREATEST(n, 3)) OVER (ORDER BY n) FROM records ORDER BY n");

    // Assert
    assert_eq!(
        aggregate.expect("aggregate argument").rows,
        vec![vec![Value::Int64(6)]]
    );
    assert_eq!(
        window.expect("window argument").rows,
        vec![vec![Value::Int64(3)]; 3]
    );
}

#[test]
fn should_describe_nullif_using_the_promoted_first_operand() {
    // Arrange
    let fixture = sql_fixture("conditional_nullif_descriptor", &[]);

    // Act
    let result = fixture.execute("SELECT NULLIF(CAST(1 AS INT), CAST(2 AS BIGINT)), NULLIF(CAST(1 AS INT), CAST(2.2 AS FLOAT))");

    // Assert
    let result = result.expect("NULLIF descriptor");
    assert_eq!(result.columns[0].type_oid, DataType::Int.type_oid());
    assert_eq!(result.columns[1].type_oid, DataType::Float.type_oid());
    assert_eq!(
        result.rows,
        vec![vec![Value::Int64(1), Value::Float64(1.0)]]
    );
}

#[test]
fn should_describe_all_null_conditional_extrema_as_text() {
    // Arrange
    let fixture = sql_fixture("conditional_null_descriptor", &[]);

    // Act
    let result = fixture.execute("SELECT GREATEST(NULL, NULL), LEAST(NULL, NULL)");

    // Assert
    let result = result.expect("all NULL descriptor");
    assert!(result
        .columns
        .iter()
        .all(|column| column.type_oid == DataType::Text.type_oid()));
    assert_eq!(result.rows, vec![vec![Value::Null, Value::Null]]);
}

#[test]
fn should_order_text_arguments_in_conditional_extrema() {
    // Arrange
    let fixture = sql_fixture("conditional_text_order", &[]);

    // Act
    let result = fixture.execute("SELECT GREATEST('a', 'z'), LEAST('a', 'z')");

    // Assert
    assert_eq!(
        result.expect("text extrema").rows,
        vec![vec![
            Value::String("z".to_string()),
            Value::String("a".to_string())
        ]]
    );
}

#[test]
fn should_evaluate_error_bearing_later_conditional_arguments() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_argument_demand",
        &[
            "CREATE TABLE records (n INT)",
            "INSERT INTO records VALUES (0)",
        ],
    );
    let statements = [
        "SELECT GREATEST(9, 1 / n) FROM records",
        "SELECT LEAST(1, 1 / n) FROM records",
        "SELECT NULLIF(9, 1 / n) FROM records",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for result in results {
        assert!(result
            .expect_err("later arguments must be evaluated")
            .to_string()
            .contains("division by zero"));
    }
}

#[test]
fn should_evaluate_second_nullif_operand_when_first_column_is_null() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_nullif_demand",
        &[
            "CREATE TABLE records (n INT, first INT)",
            "INSERT INTO records VALUES (0, NULL)",
        ],
    );

    // Act
    let result = fixture.execute("SELECT NULLIF(first, 1 / n) FROM records");

    // Assert
    assert!(result
        .expect_err("second operand is evaluated")
        .to_string()
        .contains("division by zero"));
}

#[test]
fn should_use_bound_parameters_in_conditional_extrema() {
    // Arrange
    let fixture = sql_fixture("conditional_parameters", &[]);
    let sql = "SELECT GREATEST(CAST($1 AS INT), CAST($2 AS BIGINT)), NULLIF(CAST($1 AS INT), CAST($2 AS BIGINT))";

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        sql,
        vec![Value::Int64(2), Value::Int64(7)],
    );

    // Assert
    let result = result.expect("conditional parameter binding");
    assert_eq!(result.rows, vec![vec![Value::Int64(7), Value::Int64(2)]]);
    assert_eq!(result.columns[0].type_oid, DataType::BigInt.type_oid());
    assert_eq!(result.columns[1].type_oid, DataType::Int.type_oid());
}

#[test]
fn should_reject_incompatible_conditional_types_before_scanning_empty_input() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_empty_validation",
        &["CREATE TABLE records (n INT, flag BOOLEAN)"],
    );
    let sql = "SELECT GREATEST(n, flag) FROM records";

    // Act
    let result = fixture.execute(sql);

    // Assert
    let error = result.expect_err("incompatible types must be rejected before scanning");
    let message = error.to_string();
    assert!(
        !message.contains("unsupported function"),
        "must reach type validation: {message}"
    );
    assert!(
        message.contains("type"),
        "expected type diagnostic: {message}"
    );
}

#[test]
fn should_promote_mixed_float_operands_before_conditional_comparison() {
    // Arrange
    let fixture = sql_fixture("conditional_float_promotion", &[]);

    // Act
    let result = fixture.execute("SELECT NULLIF(CAST(9007199254740993 AS BIGINT), CAST(9007199254740992 AS FLOAT)), GREATEST(CAST(9007199254740993 AS BIGINT), CAST(9007199254740992 AS FLOAT))");

    // Assert
    let result = result.expect("FLOAT equality promotes operands");
    assert_eq!(
        result.rows,
        vec![vec![Value::Null, Value::Float64(9_007_199_254_740_992.0)]]
    );
    assert!(result
        .columns
        .iter()
        .all(|column| column.type_oid == DataType::Float.type_oid()));
}

#[test]
fn should_use_conditional_boolean_results_as_predicates() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_boolean_predicate",
        &[
            "CREATE TABLE records (n INT, flag BOOLEAN)",
            "INSERT INTO records VALUES (1, false), (2, true), (3, NULL)",
        ],
    );

    // Act
    let result = fixture.execute("SELECT n FROM records WHERE LEAST(flag, true) ORDER BY n");

    // Assert
    assert_eq!(
        result.expect("BOOLEAN extrema predicate").rows,
        vec![vec![Value::Int64(2)], vec![Value::Int64(3)]]
    );
}

#[test]
fn should_promote_conditional_values_against_null_float_columns() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_null_float_column",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
        ],
    );
    let statements = [
        "SELECT NULLIF(n, f), GREATEST(n, f), LEAST(n, f) FROM records",
        "SELECT SUM(NULLIF(n, f)), SUM(GREATEST(n, f)), SUM(LEAST(n, f)) FROM records",
        "SELECT FIRST_VALUE(NULLIF(n, f)) OVER (ORDER BY n), FIRST_VALUE(GREATEST(n, f)) OVER (ORDER BY n), FIRST_VALUE(LEAST(n, f)) OVER (ORDER BY n) FROM records",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for result in results {
        let result = result.expect("NULL FLOAT type determines coercion");
        assert_eq!(
            result.rows,
            vec![vec![Value::Float64(9_007_199_254_740_992.0); 3]]
        );
        assert!(result
            .columns
            .iter()
            .all(|column| column.type_oid == DataType::Float.type_oid()));
    }
}

#[test]
fn should_promote_conditional_operands_in_join_predicates() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_join_promotion",
        &[
            "CREATE TABLE records (n BIGINT)",
            "CREATE TABLE others (f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993)",
            "INSERT INTO others VALUES (NULL)",
        ],
    );

    // Act
    let result = fixture.execute("SELECT records.n FROM records JOIN others ON NULLIF(records.n, others.f) = CAST(9007199254740992 AS FLOAT)");

    // Assert
    assert_eq!(
        result.expect("conditional JOIN operand promotion").rows,
        vec![vec![Value::Int64(9_007_199_254_740_993)]]
    );
}

#[test]
fn should_promote_conditional_operands_in_mutation_returning() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_returning_promotion",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
        ],
    );

    // Act
    let result = fixture.execute("UPDATE records SET n = n RETURNING NULLIF(n, f)");

    // Assert
    let result = result.expect("conditional RETURNING operand promotion");
    assert_eq!(
        result.rows,
        vec![vec![Value::Float64(9_007_199_254_740_992.0)]]
    );
    assert_eq!(result.columns[0].type_oid, DataType::Float.type_oid());
}

#[test]
fn should_resolve_conditional_types_for_qualified_join_sources() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_qualified_join_types",
        &[
            "CREATE TABLE records (n BIGINT)",
            "CREATE TABLE others (n FLOAT)",
            "INSERT INTO records VALUES (9007199254740993)",
            "INSERT INTO others VALUES (NULL)",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT GREATEST(records.n, others.n) FROM records CROSS JOIN others");

    // Assert
    let result = result.expect("qualified source types remain distinct");
    assert_eq!(
        result.rows,
        vec![vec![Value::Float64(9_007_199_254_740_992.0)]]
    );
    assert_eq!(result.columns[0].type_oid, DataType::Float.type_oid());
}

#[test]
fn should_promote_conditional_operands_in_dependent_cte_bodies() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_cte_types",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
        ],
    );

    // Act
    let result = fixture.execute("WITH seed AS (SELECT n, f FROM records), projected AS (SELECT GREATEST(n, f) AS value FROM seed) SELECT value FROM projected");

    // Assert
    let result = result.expect("dependent CTE coercion uses outer source types");
    assert_eq!(
        result.rows,
        vec![vec![Value::Float64(9_007_199_254_740_992.0)]]
    );
    assert_eq!(result.columns[0].type_oid, DataType::Float.type_oid());
}

#[test]
fn should_reject_excluded_conditional_argument_families_during_binding() {
    // Arrange
    let fixture = sql_fixture("conditional_excluded_types", &[]);
    let sql = "SELECT GREATEST(CAST(NULL AS DATE))";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::Unsupported(message)) if message.contains("selected conditional expression contract"))
    );
}

#[test]
fn should_observe_cancellation_before_conditional_argument_errors() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_cancellation",
        &[
            "CREATE TABLE records (n INT)",
            "INSERT INTO records VALUES (0)",
        ],
    );
    let cancellation = cassie::runtime::QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let result = fixture.cassie.execute_sql_with_cancellation(
        &fixture.session,
        "SELECT GREATEST(9, 1 / n) FROM records",
        vec![],
        &cancellation,
    );

    // Assert
    assert!(matches!(
        result,
        Err(cassie::app::CassieError::QueryCancelled)
    ));
}

#[test]
fn should_preserve_atomic_result_admission_for_conditional_queries() {
    // Arrange
    let mut config = cassie::config::CassieRuntimeConfig::default();
    config.limits.max_result_rows = 1;
    let fixture = super::support_sql_fixture::sql_fixture_with_config(
        "conditional_result_cap",
        &[
            "CREATE TABLE records (n INT)",
            "INSERT INTO records VALUES (1), (2)",
        ],
        config,
    );

    // Act
    let result = fixture.execute("SELECT GREATEST(n, 0) FROM records");

    // Assert
    assert!(matches!(
        result,
        Err(cassie::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_promote_conditional_operands_in_mutation_predicates() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_mutation_predicate",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "UPDATE records SET n = 1 WHERE NULLIF(n, f) = CAST(9007199254740992 AS FLOAT) RETURNING n",
    );

    // Assert
    assert_eq!(
        result
            .expect("conditional mutation predicate coercion")
            .rows,
        vec![vec![Value::Int64(1)]]
    );
}

#[test]
fn should_promote_conditional_operands_in_boolean_assignments() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_mutation_assignment",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT, flag BOOLEAN)",
            "INSERT INTO records VALUES (9007199254740993, NULL, false)",
        ],
    );

    // Act
    let result = fixture.execute(
        "UPDATE records SET flag = NULLIF(n, f) = CAST(9007199254740992 AS FLOAT) RETURNING flag",
    );

    // Assert
    assert_eq!(
        result
            .expect("conditional mutation assignment coercion")
            .rows,
        vec![vec![Value::Bool(true)]]
    );
}

#[test]
fn should_reject_invalid_conditional_arity_before_scanning_empty_input() {
    // Arrange
    let fixture = sql_fixture("conditional_empty_arity", &["CREATE TABLE records (n INT)"]);
    let statements = [
        "SELECT NULLIF(n) FROM records",
        "SELECT NULLIF(n, 0, 1) FROM records",
        "SELECT GREATEST() FROM records",
        "SELECT LEAST() FROM records",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for result in results {
        assert!(
            matches!(result, Err(cassie::app::CassieError::Planner(message)) if !message.contains("unsupported function"))
        );
    }
}

#[test]
fn should_promote_conditional_outer_operands_in_correlated_exists() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_correlated_outer_types",
        &[
            "CREATE TABLE records (n BIGINT)",
            "CREATE TABLE others (f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993)",
            "INSERT INTO others VALUES (NULL)",
        ],
    );

    // Act
    let result = fixture.execute("SELECT n FROM records WHERE EXISTS(SELECT 1 FROM others WHERE NULLIF(records.n, others.f) = CAST(9007199254740992 AS FLOAT))");

    // Assert
    assert_eq!(
        result.expect("conditional correlated outer coercion").rows,
        vec![vec![Value::Int64(9_007_199_254_740_993)]]
    );
}

#[test]
fn should_promote_nested_coalesce_values_for_conditional_float_results() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_nested_coalesce_float",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
        ],
    );

    // Act
    let result = fixture.execute("SELECT NULLIF(COALESCE(n, f), f), GREATEST(COALESCE(n, f), f), LEAST(COALESCE(n, f), f) FROM records");

    // Assert
    assert_eq!(
        result.expect("conditional nested float coercion").rows,
        vec![vec![Value::Float64(9_007_199_254_740_992.0); 3]]
    );
}

#[test]
fn should_promote_derived_float_carriers_for_conditional_results() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_derived_float_carriers",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
        ],
    );

    // Act
    let result = fixture.execute("WITH derived AS (SELECT COALESCE(n, f) AS value, f FROM records) SELECT NULLIF(value, f), GREATEST(value, f), LEAST(value, f) FROM derived");

    // Assert
    assert_eq!(
        result.expect("conditional derived float coercion").rows,
        vec![vec![Value::Float64(9_007_199_254_740_992.0); 3]]
    );
}

#[test]
fn should_preserve_native_nonfinite_conditional_float_values() {
    // Arrange
    let fixture = sql_fixture("conditional_native_nonfinite", &[]);

    // Act
    let result = fixture.execute("SELECT GREATEST(1e308 * 10, 0), LEAST(-1e308 * 10, 0)");

    // Assert
    assert_eq!(
        result.expect("native nonfinite float normalization").rows,
        vec![vec![
            Value::Float64(f64::INFINITY),
            Value::Float64(f64::NEG_INFINITY)
        ]]
    );
}

#[test]
fn should_preserve_explicit_inner_float_cast_errors_in_conditionals() {
    // Arrange
    let fixture = sql_fixture("conditional_explicit_nonfinite_cast", &[]);

    // Act
    let result = fixture.execute("SELECT GREATEST(CAST(1e308 * 10 AS FLOAT), 0)");

    // Assert
    assert!(result
        .expect_err("explicit inner FLOAT cast error")
        .to_string()
        .contains("cannot cast value to FLOAT"));
}

#[test]
fn should_normalize_conditional_operands_once_in_nested_cte_scopes() {
    // Arrange
    let fixture = sql_fixture("conditional_nested_cte_nonfinite", &[]);

    // Act
    let result = fixture.execute("WITH first_scope AS (SELECT GREATEST(1e308 * 10, 0) AS value), second_scope AS (SELECT LEAST(value, value) AS value FROM first_scope) SELECT NULLIF(value, 0) FROM (SELECT value FROM second_scope) AS derived");

    // Assert
    assert_eq!(
        result.expect("single conditional normalization pass").rows,
        vec![vec![Value::Float64(f64::INFINITY)]]
    );
}

#[test]
fn should_preserve_promoted_coalesce_values_across_expression_consumers() {
    // Arrange
    let fixture = sql_fixture(
        "conditional_coalesce_arithmetic",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
        ],
    );

    // Act
    let direct = fixture.execute("SELECT NULLIF(COALESCE(n,f)-1,9007199254740991.0), GREATEST(COALESCE(n,f)-1,0.0), LEAST(COALESCE(n,f)-1,9007199254740992.0) FROM records");
    let aggregate = fixture
        .execute("SELECT SUM(COALESCE(n,f)-1), SUM(GREATEST(COALESCE(n,f)-1,0.0)) FROM records");
    let window = fixture.execute("SELECT FIRST_VALUE(LEAST(COALESCE(n,f)-1,9007199254740992.0)) OVER (ORDER BY n) FROM records");

    // Assert
    assert_eq!(
        direct.expect("direct conditional consumers").rows,
        vec![vec![
            Value::Null,
            Value::Float64(9_007_199_254_740_991.0),
            Value::Float64(9_007_199_254_740_991.0)
        ]]
    );
    assert_eq!(
        aggregate.expect("aggregate conditional consumers").rows,
        vec![vec![
            Value::Float64(9_007_199_254_740_991.0),
            Value::Float64(9_007_199_254_740_991.0)
        ]]
    );
    assert_eq!(
        window.expect("window conditional consumer").rows,
        vec![vec![Value::Float64(9_007_199_254_740_991.0)]]
    );
}
