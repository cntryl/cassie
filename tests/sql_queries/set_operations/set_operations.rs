//! PostgreSQL 18 set-operation and FETCH grammar contracts tracked by #800.

use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn duplicate_set_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_800_set_left (value TEXT)",
            "CREATE TABLE issue_800_set_right (value TEXT)",
            "INSERT INTO issue_800_set_left VALUES ('a'), ('a'), ('a'), ('b')",
            "INSERT INTO issue_800_set_right VALUES ('a'), ('a'), ('c')",
        ],
    )
}

fn page_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_800_scores (score BIGINT)",
            "INSERT INTO issue_800_scores VALUES (10), (10), (9), (9), (8)",
        ],
    )
}

#[test]
fn should_apply_intersect_all_as_multiset_intersection() {
    // Arrange
    let fixture = duplicate_set_fixture("issue-800-intersect-all");

    // Act
    let result = fixture.execute(
        "SELECT value FROM issue_800_set_left INTERSECT ALL SELECT value FROM issue_800_set_right ORDER BY value",
    );

    // Assert
    assert_eq!(
        result.expect("INTERSECT ALL").rows(),
        &[
            vec![Value::String("a".to_owned())],
            vec![Value::String("a".to_owned())],
        ]
    );
}

#[test]
fn should_apply_except_all_as_multiset_subtraction() {
    // Arrange
    let fixture = duplicate_set_fixture("issue-800-except-all");

    // Act
    let result = fixture.execute(
        "SELECT value FROM issue_800_set_left EXCEPT ALL SELECT value FROM issue_800_set_right ORDER BY value",
    );

    // Assert
    assert_eq!(
        result.expect("EXCEPT ALL").rows(),
        &[
            vec![Value::String("a".to_owned())],
            vec![Value::String("b".to_owned())],
        ]
    );
}

#[test]
fn should_apply_explicit_union_distinct_to_duplicate_rows() {
    // Arrange
    let fixture = duplicate_set_fixture("issue-800-union-distinct");

    // Act
    let result = fixture.execute(
        "SELECT value FROM issue_800_set_left UNION DISTINCT SELECT value FROM issue_800_set_right ORDER BY value",
    );

    // Assert
    assert_eq!(
        result.expect("UNION DISTINCT").rows(),
        &[
            vec![Value::String("a".to_owned())],
            vec![Value::String("b".to_owned())],
            vec![Value::String("c".to_owned())],
        ]
    );
}

#[test]
fn should_apply_explicit_intersect_distinct_to_duplicate_rows() {
    // Arrange
    let fixture = duplicate_set_fixture("issue-800-intersect-distinct");

    // Act
    let result = fixture.execute(
        "SELECT value FROM issue_800_set_left INTERSECT DISTINCT SELECT value FROM issue_800_set_right ORDER BY value",
    );

    // Assert
    assert_eq!(
        result.expect("INTERSECT DISTINCT").rows(),
        &[vec![Value::String("a".to_owned())]]
    );
}

#[test]
fn should_apply_explicit_except_distinct_to_duplicate_rows() {
    // Arrange
    let fixture = duplicate_set_fixture("issue-800-except-distinct");

    // Act
    let result = fixture.execute(
        "SELECT value FROM issue_800_set_left EXCEPT DISTINCT SELECT value FROM issue_800_set_right ORDER BY value",
    );

    // Assert
    assert_eq!(
        result.expect("EXCEPT DISTINCT").rows(),
        &[vec![Value::String("b".to_owned())]]
    );
}

#[test]
fn should_preserve_duplicates_for_select_all() {
    // Arrange
    let fixture = duplicate_set_fixture("issue-800-select-all");

    // Act
    let result = fixture.execute("SELECT ALL value FROM issue_800_set_left ORDER BY value");

    // Assert
    assert_eq!(
        result.expect("SELECT ALL").rows(),
        &[
            vec![Value::String("a".to_owned())],
            vec![Value::String("a".to_owned())],
            vec![Value::String("a".to_owned())],
            vec![Value::String("b".to_owned())],
        ]
    );
}

#[test]
fn should_use_values_statements_as_set_operation_branches() {
    // Arrange
    let fixture = sql_fixture("issue-800-values-set-branches", &[]);

    // Act
    let result = fixture.execute("VALUES (2), (1) UNION ALL VALUES (2), (3) ORDER BY column1");

    // Assert
    assert_eq!(
        result.expect("VALUES set-operation branches").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(2)],
            vec![Value::Int64(3)],
        ]
    );
}

#[test]
fn should_order_each_parenthesized_set_branch_before_limiting_it() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-set-branch-ordering",
        &[
            "CREATE TABLE issue_800_branch_rows (value BIGINT)",
            "INSERT INTO issue_800_branch_rows VALUES (1), (2), (3), (4), (5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "(SELECT value FROM issue_800_branch_rows ORDER BY value DESC LIMIT 2) UNION ALL (SELECT value FROM issue_800_branch_rows ORDER BY value ASC LIMIT 2) ORDER BY value",
    );

    // Assert
    assert_eq!(
        result
            .expect("locally ordered set-operation branches")
            .rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(4)],
            vec![Value::Int64(5)],
        ]
    );
}

#[test]
fn should_order_values_using_a_selected_operator() {
    // Arrange
    let fixture = page_fixture("issue-800-order-by-using");

    // Act
    let result = fixture.execute("SELECT score FROM issue_800_scores ORDER BY score USING >");

    // Assert
    assert_eq!(
        result.expect("ORDER BY expression USING operator").rows(),
        &[
            vec![Value::Int64(10)],
            vec![Value::Int64(10)],
            vec![Value::Int64(9)],
            vec![Value::Int64(9)],
            vec![Value::Int64(8)],
        ]
    );
}

#[test]
fn should_apply_offset_row_singular_syntax() {
    // Arrange
    let fixture = page_fixture("issue-800-offset-row");

    // Act
    let result = fixture.execute(
        "SELECT score FROM issue_800_scores ORDER BY score DESC OFFSET 1 ROW FETCH FIRST 2 ROWS ONLY",
    );

    // Assert
    assert_eq!(
        result.expect("OFFSET ROW and FETCH FIRST").rows(),
        &[vec![Value::Int64(10)], vec![Value::Int64(9)]]
    );
}

#[test]
fn should_apply_offset_rows_plural_syntax() {
    // Arrange
    let fixture = page_fixture("issue-800-offset-rows");

    // Act
    let result = fixture.execute(
        "SELECT score FROM issue_800_scores ORDER BY score DESC OFFSET 2 ROWS FETCH NEXT 2 ROWS ONLY",
    );

    // Assert
    assert_eq!(
        result.expect("OFFSET ROWS and FETCH NEXT").rows(),
        &[vec![Value::Int64(9)], vec![Value::Int64(9)]]
    );
}

#[test]
fn should_fetch_one_row_when_first_count_is_omitted() {
    // Arrange
    let fixture = page_fixture("issue-800-fetch-default-count");

    // Act
    let result = fixture
        .execute("SELECT score FROM issue_800_scores ORDER BY score DESC FETCH FIRST ROW ONLY");

    // Assert
    assert_eq!(
        result.expect("FETCH FIRST ROW ONLY").rows(),
        &[vec![Value::Int64(10)]]
    );
}

#[test]
fn should_include_tied_rows_after_fetch_first_count() {
    // Arrange
    let fixture = page_fixture("issue-800-fetch-with-ties");

    // Act
    let result = fixture.execute(
        "SELECT score FROM issue_800_scores ORDER BY score DESC FETCH FIRST 1 ROWS WITH TIES",
    );

    // Assert
    assert_eq!(
        result.expect("FETCH WITH TIES").rows(),
        &[vec![Value::Int64(10)], vec![Value::Int64(10)]]
    );
}

#[test]
fn should_include_ties_at_the_fetch_boundary_after_offset() {
    // Arrange
    let fixture = page_fixture("issue-800-fetch-ties-offset");

    // Act
    let result = fixture.execute(
        "SELECT score FROM issue_800_scores ORDER BY score DESC OFFSET 1 ROWS FETCH NEXT 2 ROWS WITH TIES",
    );

    // Assert
    assert_eq!(
        result.expect("FETCH WITH TIES after OFFSET").rows(),
        &[
            vec![Value::Int64(10)],
            vec![Value::Int64(9)],
            vec![Value::Int64(9)]
        ]
    );
}

#[test]
fn should_bind_parenthesized_arithmetic_pagination_expressions_to_fetch_clauses() {
    // Arrange
    let fixture = page_fixture("issue-800-fetch-expression-parameters");

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT score FROM issue_800_scores ORDER BY score DESC OFFSET ($1 + 1) ROWS FETCH NEXT $2 ROWS ONLY",
        vec![Value::Int64(0), Value::Int64(2)],
    );

    // Assert
    assert_eq!(
        result
            .expect("parameterized OFFSET and FETCH bounds")
            .rows(),
        &[vec![Value::Int64(10)], vec![Value::Int64(9)]]
    );
}
