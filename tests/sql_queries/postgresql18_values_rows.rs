//! PostgreSQL 18 VALUES and row-expression contracts tracked by #800.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_return_default_column_names_from_standalone_values() {
    // Arrange
    let fixture = sql_fixture("issue-800-values-default-names", &[]);

    // Act
    let result = fixture.execute("VALUES (1, 'alpha'), (2, 'beta')");

    // Assert
    let result = result.expect("standalone VALUES statement");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["column1", "column2"]
    );
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::String("alpha".to_owned())],
            vec![Value::Int64(2), Value::String("beta".to_owned())],
        ]
    );
}

#[test]
fn should_bind_typed_values_rows_to_relation_column_aliases() {
    // Arrange
    let fixture = sql_fixture("issue-800-values-derived-aliases", &[]);

    // Act
    let result = fixture.execute(
        "SELECT id, label FROM (VALUES (1::BIGINT, 'alpha'), (2::BIGINT, NULL)) AS v(id, label) ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("typed VALUES relation").rows(),
        &[
            vec![Value::Int64(1), Value::String("alpha".to_owned())],
            vec![Value::Int64(2), Value::Null],
        ]
    );
}

#[test]
fn should_infer_common_types_for_mixed_values_rows() {
    // Arrange
    let fixture = sql_fixture("issue-800-values-common-type", &[]);

    // Act
    let result = fixture.execute(
        "SELECT value FROM (VALUES (1), (9007199254740993::BIGINT)) AS v(value) ORDER BY value",
    );

    // Assert
    assert_eq!(
        result.expect("mixed INT and BIGINT VALUES rows").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(9_007_199_254_740_993)]
        ]
    );
}

#[test]
fn should_preserve_explicit_bigint_identity_in_values_descriptors() {
    // Arrange
    let fixture = sql_fixture("issue-800-values-bigint-descriptor", &[]);

    // Act
    let result = fixture.execute("VALUES (9007199254740993::BIGINT)");

    // Assert
    let result = result.expect("BIGINT VALUES descriptor");
    assert_eq!(result.columns()[0].type_oid, 20);
    assert_eq!(result.rows(), &[vec![Value::Int64(9_007_199_254_740_993)]]);
}

#[test]
fn should_preserve_null_values_with_an_inferred_column_type() {
    // Arrange
    let fixture = sql_fixture("issue-800-values-null-type", &[]);

    // Act
    let result = fixture.execute("VALUES (NULL), (1)");

    // Assert
    let result = result.expect("NULL and non-NULL VALUES rows");
    assert_eq!(result.columns()[0].type_oid, 23);
    assert_eq!(result.rows(), &[vec![Value::Null], vec![Value::Int64(1)]]);
}

#[test]
fn should_preserve_bigint_type_for_an_empty_values_relation() {
    // Arrange
    let fixture = sql_fixture("issue-800-empty-values-type", &[]);

    // Act
    let result = fixture.execute("SELECT * FROM (VALUES (NULL::BIGINT)) AS v(value) WHERE false");

    // Assert
    let result = result.expect("empty typed VALUES relation");
    assert_eq!(result.columns()[0].type_oid, 20);
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
}

#[test]
fn should_preserve_duplicate_rows_in_values_relations() {
    // Arrange
    let fixture = sql_fixture("issue-800-values-duplicate-rows", &[]);

    // Act
    let result = fixture.execute("VALUES (1), (1), (2)");

    // Assert
    assert_eq!(
        result.expect("duplicate VALUES rows").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(1)],
            vec![Value::Int64(2)]
        ]
    );
}

#[test]
fn should_order_values_rows_by_default_names_with_pagination() {
    // Arrange
    let fixture = sql_fixture("issue-800-values-pagination", &[]);

    // Act
    let result = fixture.execute("VALUES (4), (1), (3), (2) ORDER BY column1 LIMIT 2 OFFSET 1");

    // Assert
    assert_eq!(
        result.expect("ordered VALUES pagination").rows(),
        &[vec![Value::Int64(2)], vec![Value::Int64(3)]]
    );
}

#[test]
fn should_use_values_as_a_common_table_expression_body() {
    // Arrange
    let fixture = sql_fixture("issue-800-values-cte-body", &[]);

    // Act
    let result = fixture.execute(
        "WITH chosen(id, label) AS (VALUES (1, 'one'), (2, 'two')) SELECT id, label FROM chosen ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("VALUES CTE body").rows(),
        &[
            vec![Value::Int64(1), Value::String("one".to_owned())],
            vec![Value::Int64(2), Value::String("two".to_owned())],
        ]
    );
}

#[test]
fn should_insert_rows_selected_from_a_values_relation() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-values-insert-source",
        &["CREATE TABLE copied_values (id BIGINT, label TEXT)"],
    );

    // Act
    let inserted = fixture.execute(
        "INSERT INTO copied_values (id, label) SELECT id, label FROM (VALUES (7::BIGINT, 'seven'), (8::BIGINT, 'eight')) AS source(id, label)",
    );
    let selected = fixture.execute("SELECT id, label FROM copied_values ORDER BY id");

    // Assert
    inserted.expect("insert from VALUES source");
    assert_eq!(
        selected.expect("rows inserted from VALUES source").rows(),
        &[
            vec![Value::Int64(7), Value::String("seven".to_owned())],
            vec![Value::Int64(8), Value::String("eight".to_owned())],
        ]
    );
}

#[test]
fn should_evaluate_row_constructor_equality() {
    // Arrange
    let fixture = sql_fixture("issue-800-row-constructor-equality", &[]);

    // Act
    let result = fixture.execute("SELECT ROW(1, 'one') = ROW(1, 'one')");

    // Assert
    assert_eq!(
        result.expect("row constructor equality").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_compare_row_constructors_lexicographically() {
    // Arrange
    let fixture = sql_fixture("issue-800-row-ordering", &[]);

    // Act
    let result = fixture
        .execute("SELECT (1, 2) < (1, 3), (1, 2) <= (1, 2), (2, 1) > (1, 9), (2, 1) >= (2, 1)");

    // Assert
    assert_eq!(
        result.expect("row ordering comparisons").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_stop_row_ordering_at_the_first_decisive_field() {
    // Arrange
    let fixture = sql_fixture("issue-800-row-ordering-null-stop", &[]);

    // Act
    let result = fixture.execute("SELECT (1, NULL) < (2, 0)");

    // Assert
    assert_eq!(
        result
            .expect("row comparison stops at a decisive field")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_return_null_when_row_ordering_reaches_a_null_field() {
    // Arrange
    let fixture = sql_fixture("issue-800-row-ordering-null-result", &[]);

    // Act
    let result = fixture.execute("SELECT (1, NULL) < (1, 2)");

    // Assert
    assert_eq!(
        result
            .expect("row comparison with unresolved NULL field")
            .rows(),
        &[vec![Value::Null]]
    );
}

#[test]
fn should_apply_distinctness_semantics_to_row_constructors() {
    // Arrange
    let fixture = sql_fixture("issue-800-row-distinctness", &[]);

    // Act
    let result = fixture.execute(
        "SELECT ROW(1, NULL) IS NOT DISTINCT FROM ROW(1, NULL), ROW(1, NULL) IS DISTINCT FROM ROW(1, 2)",
    );

    // Assert
    assert_eq!(
        result.expect("row IS DISTINCT FROM predicates").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_null_row_equality_distinctness_semantics() {
    // Arrange
    let fixture = sql_fixture("issue-800-row-null-equality", &[]);

    // Act
    let result = fixture.execute(
        "SELECT (1, NULL) = (1, NULL), (1, NULL) <> (1, NULL), (1, NULL) IS DISTINCT FROM (1, NULL), (1, NULL) IS NOT DISTINCT FROM (1, NULL)",
    );

    // Assert
    let result = result.expect("NULL row equality and distinctness");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Null,
            Value::Null,
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16, 16, 16]
    );
}

#[test]
fn should_expose_merged_using_columns_to_projection() {
    // Arrange
    let fixture = sql_fixture("issue-800-using-merged-columns", &[]);

    // Act
    let merged = fixture.execute(
        "SELECT * FROM (VALUES (1::BIGINT, 'left')) AS l(id, left_value) JOIN (VALUES (1::BIGINT, 'right')) AS r(id, right_value) USING (id)",
    );
    let aliased = fixture.execute(
        "SELECT merged.id FROM (VALUES (1::BIGINT)) AS l(id) JOIN (VALUES (1::BIGINT)) AS r(id) USING (id) AS merged",
    );

    // Assert
    let merged = merged.expect("USING exposes a single merged column");
    assert_eq!(
        merged
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "left_value", "right_value"]
    );
    assert_eq!(
        merged.rows(),
        &[vec![
            Value::Int64(1),
            Value::String("left".to_owned()),
            Value::String("right".to_owned()),
        ]]
    );
    assert_eq!(
        aliased
            .expect("USING alias exposes the merged column")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_expand_qualified_stars_inside_row_constructors() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-row-qualified-star",
        &[
            "CREATE TABLE row_star_source (id BIGINT, label TEXT)",
            "INSERT INTO row_star_source VALUES (1, 'one')",
        ],
    );

    // Act
    let result = fixture
        .execute("SELECT ROW(source.*) = ROW(1::BIGINT, 'one') FROM row_star_source AS source");

    // Assert
    assert_eq!(
        result.expect("ROW with qualified star").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_expand_qualified_stars_before_later_row_fields() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-row-qualified-star-tail",
        &[
            "CREATE TABLE row_star_tail_source (id BIGINT, label TEXT)",
            "INSERT INTO row_star_tail_source VALUES (1, 'one')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT ROW(source.*, 42) = ROW(1::BIGINT, 'one', 42) FROM row_star_tail_source AS source",
    );

    // Assert
    assert_eq!(
        result
            .expect("ROW with qualified star and trailing field")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}
