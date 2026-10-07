use super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_promote_coalesce_before_downstream_arithmetic() {
    // Arrange
    let fixture = sql_fixture(
        "coalesce-promotion-boundary",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT COALESCE(n, f), COALESCE(n, f) - 1 FROM records");

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Float64(9_007_199_254_740_992.0),
            Value::Float64(9_007_199_254_740_991.0),
        ]]
    );
}

#[test]
fn should_preserve_coalesce_promotion_through_derived_and_cte_scopes() {
    // Arrange
    let fixture = sql_fixture(
        "coalesce-promotion-scopes",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
            "CREATE VIEW promoted AS SELECT COALESCE(n, f) AS v FROM records",
        ],
    );
    let queries = [
        "WITH c AS (SELECT COALESCE(n, f) AS v FROM records) SELECT v - 1 FROM c",
        "SELECT v - 1 FROM (SELECT COALESCE(n, f) AS v FROM records) AS d",
        "WITH c AS (SELECT n, f FROM records) SELECT COALESCE(n, f) - 1 FROM c",
        "SELECT COALESCE(n, f) - 1 FROM (SELECT n, f FROM records) AS d",
        "SELECT CASE WHEN TRUE THEN COALESCE(n, f) - 1 ELSE 0.0 END FROM records",
        "SELECT COALESCE(COALESCE(n, f), 0.0) - 1 FROM records",
        "SELECT v - 1 FROM promoted",
        "SELECT COALESCE(n, f) - 1 FROM records WHERE EXISTS (SELECT n FROM records WHERE COALESCE(n, f) - 1 = 9007199254740991)",
    ];

    // Act
    let rows = queries.map(|sql| {
        fixture
            .execute(sql)
            .unwrap_or_else(|error| panic!("query failed: {sql}: {error}"))
            .rows
    });

    // Assert
    for (sql, rows) in queries.into_iter().zip(rows) {
        assert_eq!(
            rows,
            vec![vec![Value::Float64(9_007_199_254_740_991.0)]],
            "{sql}"
        );
    }
}

#[test]
fn should_preserve_coalesce_demand_nulls_and_explicit_cast_errors() {
    // Arrange
    let fixture = sql_fixture(
        "coalesce-promotion-demand",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT, z INT)",
            "INSERT INTO records VALUES (9007199254740993, NULL, 0)",
        ],
    );

    // Act
    let lazy = fixture.rows("SELECT COALESCE(n, CAST(1 / z AS FLOAT)) - 1 FROM records");
    let nulls = fixture.rows("SELECT COALESCE(CAST(NULL AS BIGINT), f) FROM records");
    let explicit_error =
        fixture.error("SELECT COALESCE(CAST(CAST(n AS TEXT) AS INT), f) FROM records");
    let integers = fixture.rows("SELECT COALESCE(n, CAST(NULL AS BIGINT)) - 1 FROM records");

    // Assert
    assert_eq!(lazy, vec![vec![Value::Float64(9_007_199_254_740_991.0)]]);
    assert_eq!(nulls, vec![vec![Value::Null]]);
    assert!(explicit_error
        .to_string()
        .contains("cannot cast value to INT"));
    assert_eq!(integers, vec![vec![Value::Int64(9_007_199_254_740_992)]]);
}

#[test]
fn should_promote_coalesce_in_mutation_returning_and_insert_projections() {
    // Arrange
    let fixture = sql_fixture(
        "coalesce-promotion-mutations",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
            "CREATE TABLE results (v FLOAT)",
        ],
    );

    // Act
    let returned =
        fixture.rows("UPDATE records SET n = n RETURNING COALESCE(n, f), COALESCE(n, f) - 1");
    fixture
        .execute("INSERT INTO results SELECT COALESCE(n, f) - 1 FROM records")
        .expect("insert projection");
    let assigned = fixture.rows("UPDATE records SET f = COALESCE(n, f) - 1 RETURNING f");
    let inserted = fixture.rows("SELECT v FROM results");

    // Assert
    assert_eq!(
        returned,
        vec![vec![
            Value::Float64(9_007_199_254_740_992.0),
            Value::Float64(9_007_199_254_740_991.0)
        ]]
    );
    assert_eq!(
        assigned,
        vec![vec![Value::Float64(9_007_199_254_740_991.0)]]
    );
    assert_eq!(
        inserted,
        vec![vec![Value::Float64(9_007_199_254_740_991.0)]]
    );
}
