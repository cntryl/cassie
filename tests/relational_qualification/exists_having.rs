use crate::support_relational_qualification::fixture;
use cassie::types::Value;

#[test]
fn should_resolve_exists_against_actual_having_groups() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE having_outer(id BIGINT)",
        "CREATE TABLE having_inner(id BIGINT,n BIGINT)",
        "INSERT INTO having_outer VALUES(1),(1),(2)",
        "INSERT INTO having_inner VALUES(1,0)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("having setup");
    }
    let queries=[
        "SELECT q.id AS id,COUNT(*) AS n FROM having_outer q GROUP BY q.id HAVING EXISTS(SELECT 1 FROM having_inner i WHERE i.id=q.id) ORDER BY q.id",
        "SELECT q.id AS id,COUNT(*) AS n FROM having_outer q GROUP BY q.id HAVING EXISTS(SELECT 1 FROM having_inner i) ORDER BY q.id",
        "SELECT q.id AS id,COUNT(*) AS n FROM having_outer q GROUP BY q.id HAVING CASE WHEN q.id=1 THEN true ELSE EXISTS(SELECT i.n / i.n FROM having_inner i WHERE i.id=q.id) END ORDER BY q.id",
    ];
    // Act
    let results = queries
        .iter()
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]))
        .collect::<Vec<_>>();
    // Assert
    for ((sql, result), index) in queries.iter().zip(results).zip(0..) {
        let result = result.unwrap_or_else(|error| panic!("{sql}: {error}"));
        let mut expected = vec![vec![Value::Int64(1), Value::Int64(2)]];
        if index == 1 {
            expected.push(vec![Value::Int64(2), Value::Int64(1)]);
        }
        assert_eq!(result.rows, expected, "{sql}");
        assert_eq!(result.columns.len(), 2);
        for (column, name) in result.columns.iter().zip(["id", "n"]) {
            assert_eq!(
                (
                    column.name.as_str(),
                    column.type_oid,
                    column.typlen,
                    column.atttypmod,
                    column.format_code
                ),
                (name, 20, 8, -1, 0)
            );
        }
    }
}

#[test]
fn should_preserve_having_scalar_selection() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE having_lazy(id BIGINT,n BIGINT)",
        "INSERT INTO having_lazy VALUES(1,0),(2,0)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("having scalar setup");
    }
    // Act
    let skipped=fixture.cassie.execute_sql(&fixture.session,
        "SELECT q.id AS id FROM having_lazy q GROUP BY q.id HAVING COALESCE(true,EXISTS(SELECT i.n / i.n FROM having_lazy i WHERE i.id=q.id)) ORDER BY q.id",vec![]);
    let reached=fixture.cassie.execute_sql(&fixture.session,
        "SELECT q.id AS id FROM having_lazy q GROUP BY q.id HAVING CASE WHEN q.id=1 THEN EXISTS(SELECT i.n / i.n FROM having_lazy i WHERE i.id=q.id) ELSE false END",vec![]);
    let empty=fixture.cassie.execute_sql(&fixture.session,
        "SELECT q.id AS id FROM having_lazy q WHERE q.id<0 GROUP BY q.id HAVING EXISTS(SELECT i.n / i.n FROM having_lazy i WHERE i.id=q.id)",vec![]);
    let eager=fixture.cassie.execute_sql(&fixture.session,
        "SELECT q.id AS id FROM having_lazy q WHERE q.id<0 GROUP BY q.id HAVING EXISTS(SELECT i.n / i.n FROM having_lazy i)",vec![]);
    // Assert
    assert_eq!(
        skipped.expect("unreached correlated branch").rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    assert_eq!(
        empty.expect("no groups to evaluate").rows,
        Vec::<Vec<Value>>::new()
    );
    for result in [reached, eager] {
        let error = result.expect_err("selected occurrence must raise division error");
        assert!(
            matches!(error,cassie::app::CassieError::Execution(ref message) if message.contains("division by zero")),
            "{error}"
        );
    }
}

#[test]
fn should_preserve_multiple_having_group_keys() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE having_groups(id BIGINT,n BIGINT)",
        "CREATE TABLE having_members(id BIGINT,n BIGINT)",
        "INSERT INTO having_groups VALUES(1,0),(1,0),(1,10),(2,0)",
        "INSERT INTO having_members VALUES(1,0)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("group key setup");
    }
    // Act
    let result=fixture.cassie.execute_sql(&fixture.session,
        "SELECT q.id AS id,q.n AS n,COUNT(*) AS total FROM having_groups q GROUP BY q.id,q.n HAVING EXISTS(SELECT 1 FROM having_members i WHERE i.id=q.id AND i.n=q.n) ORDER BY q.id,q.n",vec![]);
    // Assert
    let result = result.expect("correlation retains both actual group keys");
    assert_eq!(
        result.rows,
        vec![vec![Value::Int64(1), Value::Int64(0), Value::Int64(2)]]
    );
    for (column, name) in result.columns.iter().zip(["id", "n", "total"]) {
        assert_eq!(
            (
                column.name.as_str(),
                column.type_oid,
                column.typlen,
                column.atttypmod,
                column.format_code
            ),
            (name, 20, 8, -1, 0)
        );
    }
    assert_eq!(result.columns.len(), 3);
}
