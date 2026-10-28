//! PostgreSQL 18 relation-source and join grammar contracts tracked by #800.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

fn join_fixture(label: &str) -> crate::support_sql_fixture::SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_800_employees (id BIGINT, dept_id BIGINT, name TEXT)",
            "CREATE TABLE issue_800_departments (dept_id BIGINT, dept_name TEXT)",
            "INSERT INTO issue_800_employees VALUES (1, 10, 'Ada'), (2, 20, 'Lin')",
            "INSERT INTO issue_800_departments VALUES (10, 'Research'), (30, 'Design')",
        ],
    )
}

#[test]
fn should_expand_qualified_stars_from_both_join_sides_with_on_predicate() {
    // Arrange
    let fixture = join_fixture("issue-800-qualified-join-stars-on");

    // Act
    let result = fixture.execute(
        "SELECT issue_800_employees.*, issue_800_departments.* FROM issue_800_employees JOIN issue_800_departments ON issue_800_employees.dept_id = issue_800_departments.dept_id",
    );

    // Assert
    let result = result.expect("qualified stars across an ON join");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "dept_id", "name", "dept_id", "dept_name"]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(1),
            Value::Int64(10),
            Value::String("Ada".to_owned()),
            Value::Int64(10),
            Value::String("Research".to_owned()),
        ]]
    );
}

#[test]
fn should_expand_qualified_stars_from_derived_relations() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-qualified-derived-star",
        &[
            "CREATE TABLE issue_800_star_source (id BIGINT, label TEXT)",
            "INSERT INTO issue_800_star_source VALUES (1, 'one')",
        ],
    );

    // Act
    let result = fixture
        .execute("SELECT derived.* FROM (SELECT id, label FROM issue_800_star_source) AS derived");

    // Assert
    let result = result.expect("qualified star over a derived relation");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "label"]
    );
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(1), Value::String("one".to_owned())]]
    );
}

#[test]
fn should_expand_qualified_stars_from_common_table_expressions() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-qualified-cte-star",
        &[
            "CREATE TABLE issue_800_cte_star_source (id BIGINT, label TEXT)",
            "INSERT INTO issue_800_cte_star_source VALUES (1, 'one')",
        ],
    );

    // Act
    let result = fixture.execute(
        "WITH chosen AS (SELECT id, label FROM issue_800_cte_star_source) SELECT chosen.* FROM chosen",
    );

    // Assert
    let result = result.expect("qualified star over a CTE");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "label"]
    );
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(1), Value::String("one".to_owned())]]
    );
}

#[test]
fn should_expand_qualified_stars_inside_common_table_expression_bodies() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-qualified-cte-body-star",
        &[
            "CREATE TABLE issue_800_cte_body_star_source (id BIGINT, label TEXT)",
            "INSERT INTO issue_800_cte_body_star_source VALUES (1, 'one')",
        ],
    );

    // Act
    let result = fixture.execute(
        "WITH chosen AS (SELECT source.* FROM issue_800_cte_body_star_source AS source) SELECT chosen.* FROM chosen",
    );

    // Assert
    let result = result.expect("qualified wildcard in a CTE body");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "label"]
    );
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(1), Value::String("one".to_owned())]]
    );
}

#[test]
fn should_preserve_qualified_wildcard_set_result() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-qualified-wildcard-set-width",
        &[
            "CREATE TABLE issue_800_set_star_source (id BIGINT, label TEXT)",
            "INSERT INTO issue_800_set_star_source VALUES (1, 'one')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT source.* FROM issue_800_set_star_source AS source UNION ALL SELECT source.* FROM issue_800_set_star_source AS source",
    );

    // Assert
    let result = result.expect("qualified wildcard set operands");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "label"]
    );
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::String("one".to_owned())],
            vec![Value::Int64(1), Value::String("one".to_owned())],
        ]
    );
}

#[test]
fn should_expand_qualified_stars_through_quoted_relation_aliases() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-qualified-quoted-alias-star",
        &[
            "CREATE TABLE issue_800_quoted_alias_star_source (id BIGINT, label TEXT)",
            "INSERT INTO issue_800_quoted_alias_star_source VALUES (1, 'one')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT \"ChosenAlias\".* FROM issue_800_quoted_alias_star_source AS \"ChosenAlias\"",
    );

    // Assert
    let result = result.expect("quoted relation qualifier is preserved");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "label"]
    );
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(1), Value::String("one".to_owned())]]
    );
}

#[test]
fn should_reject_unknown_qualified_wildcard_sources() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-qualified-unknown-star",
        &[
            "CREATE TABLE issue_800_unknown_star_source (id BIGINT)",
            "INSERT INTO issue_800_unknown_star_source VALUES (1)",
        ],
    );

    // Act
    let error = fixture
        .execute("SELECT missing.* FROM issue_800_unknown_star_source")
        .expect_err("an unknown wildcard qualifier must fail binding");

    // Assert
    assert!(error.to_string().contains("unknown relation qualifier"));
}

#[test]
fn should_reject_order_ordinals_with_qualified_stars() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-qualified-star-order-ordinal",
        &[
            "CREATE TABLE issue_800_star_ordinal_source (id BIGINT)",
            "INSERT INTO issue_800_star_ordinal_source VALUES (1)",
        ],
    );

    // Act
    let error = fixture
        .execute("SELECT source.* FROM issue_800_star_ordinal_source AS source ORDER BY 1")
        .expect_err("the ordinal must not address an unexpanded wildcard slot");

    // Assert
    assert!(error.to_string().contains("not supported with SELECT *"));
}

#[test]
fn should_select_a_table_as_a_query_source() {
    // Arrange
    let fixture = join_fixture("issue-800-table-query");

    // Act
    let result = fixture.execute("TABLE issue_800_employees ORDER BY id");

    // Assert
    assert_eq!(
        result.expect("TABLE query shorthand").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(10),
                Value::String("Ada".to_owned()),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(20),
                Value::String("Lin".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_use_table_queries_as_cte_bodies() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-table-cte-body",
        &[
            "CREATE TABLE issue_800_table_cte_source (id BIGINT)",
            "INSERT INTO issue_800_table_cte_source VALUES (4), (5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "WITH chosen AS (TABLE issue_800_table_cte_source) SELECT * FROM chosen ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("TABLE CTE body").rows(),
        &[vec![Value::Int64(4)], vec![Value::Int64(5)]]
    );
}

#[test]
fn should_use_table_queries_as_derived_relations() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-table-derived-source",
        &[
            "CREATE TABLE issue_800_table_derived_source (id BIGINT)",
            "INSERT INTO issue_800_table_derived_source VALUES (6)",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT source.* FROM (TABLE issue_800_table_derived_source) AS source");

    // Assert
    let result = result.expect("TABLE derived relation");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id"]
    );
    assert_eq!(result.rows(), &[vec![Value::Int64(6)]]);
}

#[test]
fn should_insert_rows_from_a_table_query_source() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-insert-table-source",
        &[
            "CREATE TABLE issue_800_insert_source (id BIGINT)",
            "CREATE TABLE issue_800_insert_target (id BIGINT)",
            "INSERT INTO issue_800_insert_source VALUES (8), (9)",
        ],
    );

    // Act
    let inserted =
        fixture.execute("INSERT INTO issue_800_insert_target TABLE issue_800_insert_source");
    let selected = fixture.execute("SELECT id FROM issue_800_insert_target ORDER BY id");

    // Assert
    inserted.expect("INSERT TABLE source");
    assert_eq!(
        selected.expect("rows copied from TABLE source").rows(),
        &[vec![Value::Int64(8)], vec![Value::Int64(9)]]
    );
}
