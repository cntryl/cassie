use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn chain_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE sa (n INT)",
            "CREATE TABLE sb (n INT)",
            "CREATE TABLE sc (n INT)",
            "CREATE TABLE sd (n INT)",
            "INSERT INTO sa (n) VALUES (1)",
            "INSERT INTO sa (n) VALUES (2)",
            "INSERT INTO sb (n) VALUES (1)",
            "INSERT INTO sc (n) VALUES (2)",
            "INSERT INTO sd (n) VALUES (3)",
        ],
    )
}

fn ints(rows: Vec<Vec<Value>>) -> Vec<i64> {
    rows.into_iter()
        .map(|row| match row.first() {
            Some(Value::Int64(value)) => *value,
            other => panic!("expected an integer cell, got {other:?}"),
        })
        .collect()
}

fn sorted(fixture: &SqlFixture, sql: &str) -> Vec<i64> {
    let mut values = ints(fixture.rows(sql));
    values.sort_unstable();
    values
}

#[test]
fn should_evaluate_mixed_set_operators_left_to_right_with_intersect_first() {
    // Arrange
    let fixture = chain_fixture("set_chain_precedence");

    // Act
    let intersect_union = sorted(
        &fixture,
        "SELECT n FROM sa INTERSECT SELECT n FROM sb UNION SELECT n FROM sd",
    );
    let except_union = sorted(
        &fixture,
        "SELECT n FROM sa EXCEPT SELECT n FROM sb UNION SELECT n FROM sd",
    );
    let union_all_union = sorted(
        &fixture,
        "SELECT n FROM sa UNION ALL SELECT n FROM sb UNION SELECT n FROM sb",
    );
    let union_intersect = sorted(
        &fixture,
        "SELECT n FROM sa UNION SELECT n FROM sb INTERSECT SELECT n FROM sd",
    );
    let union_all_intersect = sorted(
        &fixture,
        "SELECT n FROM sa UNION ALL SELECT n FROM sb UNION ALL \
             SELECT n FROM sb INTERSECT SELECT n FROM sa",
    );

    // Assert
    assert_eq!(intersect_union, vec![1, 3]);
    assert_eq!(except_union, vec![2, 3]);
    assert_eq!(union_all_union, vec![1, 2]);
    assert_eq!(union_intersect, vec![1, 2]);
    assert_eq!(union_all_intersect, vec![1, 1, 1, 2]);
}

#[test]
fn should_accept_intersect_or_except_after_the_first_set_operator() {
    // Arrange
    let fixture = chain_fixture("set_chain_second_operator");

    // Act
    let except_except = sorted(
        &fixture,
        "SELECT n FROM sa EXCEPT SELECT n FROM sb EXCEPT SELECT n FROM sc",
    );
    let intersect_intersect = sorted(
        &fixture,
        "SELECT n FROM sa INTERSECT SELECT n FROM sa INTERSECT SELECT n FROM sb",
    );
    let union_except = sorted(
        &fixture,
        "SELECT n FROM sa UNION SELECT n FROM sb EXCEPT SELECT n FROM sb",
    );

    // Assert
    assert_eq!(except_except, Vec::<i64>::new());
    assert_eq!(intersect_intersect, vec![1]);
    assert_eq!(union_except, vec![2]);
}

#[test]
fn should_resolve_global_clauses_over_a_regrouped_set_chain() {
    // Arrange
    let fixture = chain_fixture("set_chain_global_clauses");

    // Act
    let ordered = ints(fixture.rows(
        "SELECT n FROM sa EXCEPT SELECT n FROM sb UNION SELECT n FROM sd \
             ORDER BY 1 DESC LIMIT 1",
    ));
    let aliased = fixture.execute(
        "SELECT n AS m FROM sa EXCEPT SELECT n FROM sb UNION SELECT n FROM sd ORDER BY m DESC",
    );
    let cte_right = sorted(
        &fixture,
        "WITH x AS (SELECT n FROM sd) \
             SELECT n FROM sa EXCEPT SELECT n FROM sb UNION SELECT n FROM x",
    );
    let cte_left = sorted(
        &fixture,
        "WITH x AS (SELECT n FROM sa) \
             SELECT n FROM x EXCEPT SELECT n FROM sb UNION SELECT n FROM sd",
    );

    let Ok(regrouped) =
        fixture.execute("SELECT n FROM sa EXCEPT SELECT n FROM sb UNION SELECT n FROM sd")
    else {
        panic!("regrouped chain failed");
    };
    let Ok(plain) = fixture.execute("SELECT n FROM sa UNION SELECT n FROM sd") else {
        panic!("plain union failed");
    };

    // Assert
    assert_eq!(regrouped.columns(), plain.columns());
    assert_eq!(ordered, vec![3]);
    let Ok(aliased) = aliased else {
        panic!("aliased regrouped chain failed");
    };
    assert_eq!(aliased.columns()[0].name, "m");
    assert_eq!(ints(aliased.rows().to_vec()), vec![3, 2]);
    assert_eq!(cte_right, vec![2, 3]);
    assert_eq!(cte_left, vec![2, 3]);
}

#[test]
fn should_order_compound_constants_by_output_ordinal() {
    // Arrange
    let fixture = sql_fixture("set_order_constant_ordinal", &[]);

    // Act
    let ordinal = ints(fixture.rows("SELECT 1 UNION SELECT 2 ORDER BY 1 DESC"));
    let named = ints(fixture.rows("SELECT 1 AS n UNION SELECT 2 ORDER BY n DESC"));

    // Assert
    assert_eq!(named, vec![2, 1]);
    assert_eq!(ordinal, named);
}

#[test]
fn should_order_compound_duplicate_names_by_position_before_pagination() {
    // Arrange
    let fixture = sql_fixture("set_order_duplicate_names", &[]);

    // Act
    let rows = fixture.rows(
        "SELECT 1 AS x, 3 + 0 AS x UNION ALL SELECT 2, 1 UNION ALL SELECT 3, 2              ORDER BY 2 DESC LIMIT 1 OFFSET 1",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(3), Value::Int64(2)]]);
}

#[test]
fn should_order_compound_nested_tails_independently() {
    // Arrange
    let fixture = sql_fixture("set_order_nested_ordinal", &[]);

    // Act
    let rows = ints(fixture.rows(
        "(SELECT 1 UNION SELECT 3 ORDER BY 1 DESC LIMIT 1) UNION ALL SELECT 2 ORDER BY 1 ASC",
    ));
    // Assert
    assert_eq!(rows, vec![2, 3]);
}

#[test]
fn should_reject_compound_order_position_outside_output() {
    // Arrange
    let fixture = sql_fixture("set_order_invalid_ordinal", &[]);

    // Act
    let result = fixture.execute("SELECT 1 UNION SELECT 2 ORDER BY 2");

    // Assert
    let error = result.expect_err("invalid order position");
    assert!(error
        .to_string()
        .contains("ORDER BY position 2 is not in select list"));
}
