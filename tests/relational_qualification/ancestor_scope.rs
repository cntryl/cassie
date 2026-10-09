use crate::support_relational_qualification::fixture;
use cassie::types::Value;

#[test]
fn should_preserve_ancestor_scope_in_nested_exists() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE nesting_outer(id BIGINT)",
        "CREATE TABLE nesting_inner(id BIGINT)",
        "INSERT INTO nesting_outer VALUES (1),(2)",
        "INSERT INTO nesting_inner VALUES (1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("nested setup");
    }
    let queries = [
        "SELECT o.id,EXISTS(SELECT 1 FROM nesting_inner i WHERE i.id=o.id) AS matched FROM nesting_outer o ORDER BY o.id",
        "SELECT o.id,EXISTS(SELECT 1 FROM nesting_outer m WHERE m.id=o.id AND EXISTS(SELECT 1 FROM nesting_inner i WHERE i.id=o.id)) AS matched FROM nesting_outer o ORDER BY o.id",
        "SELECT o.id FROM nesting_outer o WHERE EXISTS(SELECT 1 FROM nesting_outer m WHERE m.id=o.id AND EXISTS(SELECT 1 FROM nesting_inner i WHERE i.id=o.id)) ORDER BY o.id",
    ];
    // Act
    let results = queries
        .iter()
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]))
        .collect::<Vec<_>>();
    // Assert
    for ((sql, result), boolean) in queries.iter().zip(results).zip([true, true, false]) {
        let result = result.unwrap_or_else(|error| panic!("{sql}: {error}"));
        let expected = if boolean {
            vec![
                vec![Value::Int64(1), Value::Bool(true)],
                vec![Value::Int64(2), Value::Bool(false)],
            ]
        } else {
            vec![vec![Value::Int64(1)]]
        };
        assert_eq!(result.rows, expected, "{sql}");
        assert_eq!(
            (
                result.columns[0].type_oid,
                result.columns[0].typlen,
                result.columns[0].atttypmod,
                result.columns[0].format_code
            ),
            (20, 8, -1, 0)
        );
        if boolean {
            assert_eq!(
                (
                    result.columns[1].name.as_str(),
                    result.columns[1].type_oid,
                    result.columns[1].typlen,
                    result.columns[1].atttypmod,
                    result.columns[1].format_code
                ),
                ("matched", 16, 1, -1, 0)
            );
        }
    }
}

#[test]
fn should_preserve_ancestor_identity_through_selected_nested_sources() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE ancestor_outer(id BIGINT)",
        "CREATE TABLE ancestor_middle(id BIGINT)",
        "CREATE TABLE ancestor_inner(id BIGINT)",
        "INSERT INTO ancestor_outer VALUES (1),(2)",
        "INSERT INTO ancestor_middle VALUES (10)",
        "INSERT INTO ancestor_inner VALUES (1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("ancestor source setup");
    }
    let queries=[
        "SELECT o.id,EXISTS(SELECT 1 FROM ancestor_middle m WHERE EXISTS(SELECT 1 FROM ancestor_inner i WHERE i.id=o.id AND m.id=10)) AS matched FROM ancestor_outer o ORDER BY o.id",
        "WITH c AS (SELECT id FROM ancestor_middle) SELECT o.id,EXISTS(SELECT 1 FROM c m WHERE EXISTS(SELECT 1 FROM ancestor_inner i WHERE i.id=o.id AND m.id=10)) AS matched FROM ancestor_outer o ORDER BY o.id",
        "SELECT o.id,EXISTS(SELECT 1 FROM (SELECT id FROM ancestor_middle) m WHERE EXISTS(SELECT 1 FROM ancestor_inner i WHERE i.id=o.id AND m.id=10)) AS matched FROM ancestor_outer o ORDER BY o.id",
        "SELECT o.id,EXISTS(SELECT EXISTS(SELECT 1 FROM ancestor_inner i WHERE i.id=o.id AND m.id=10) FROM ancestor_middle m WHERE m.id=10) AS matched FROM ancestor_outer o ORDER BY o.id",
    ];
    // Act
    let results = queries
        .iter()
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]))
        .collect::<Vec<_>>();
    // Assert
    for ((sql, result), index) in queries.iter().zip(results).zip(0..) {
        let result = result.unwrap_or_else(|error| panic!("{sql}: {error}"));
        let expected = if index == 3 {
            vec![
                vec![Value::Int64(1), Value::Bool(true)],
                vec![Value::Int64(2), Value::Bool(true)],
            ]
        } else {
            vec![
                vec![Value::Int64(1), Value::Bool(true)],
                vec![Value::Int64(2), Value::Bool(false)],
            ]
        };
        assert_eq!(result.rows, expected, "{sql}");
        assert_eq!(
            (
                result.columns[1].name.as_str(),
                result.columns[1].type_oid,
                result.columns[1].typlen,
                result.columns[1].atttypmod,
                result.columns[1].format_code
            ),
            ("matched", 16, 1, -1, 0)
        );
    }
}

#[test]
fn should_preserve_nested_exists_branch_selection() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE lazy_ancestor(id BIGINT,n BIGINT)",
        "INSERT INTO lazy_ancestor VALUES (1,0),(2,0)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("nested lazy setup");
    }
    let queries=[
        "SELECT o.id,EXISTS(SELECT CASE WHEN m.id=1 THEN false ELSE EXISTS(SELECT i.n / i.n FROM lazy_ancestor i WHERE i.id=o.id) END FROM lazy_ancestor m WHERE m.id=1) AS matched FROM lazy_ancestor o ORDER BY o.id",
        "SELECT o.id,EXISTS(SELECT COALESCE(true,EXISTS(SELECT i.n / i.n FROM lazy_ancestor i WHERE i.id=o.id)) FROM lazy_ancestor m WHERE m.id=1) AS matched FROM lazy_ancestor o ORDER BY o.id",
        "SELECT o.id,EXISTS(SELECT 1 FROM lazy_ancestor m WHERE m.id=2 AND EXISTS(SELECT 1 FROM lazy_ancestor o WHERE o.id=1)) AS matched FROM lazy_ancestor o ORDER BY o.id",
    ];
    // Act
    let results = queries
        .iter()
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]))
        .collect::<Vec<_>>();
    // Assert
    for (sql, result) in queries.iter().zip(results) {
        assert_eq!(
            result.unwrap_or_else(|error| panic!("{sql}: {error}")).rows,
            vec![
                vec![Value::Int64(1), Value::Bool(true)],
                vec![Value::Int64(2), Value::Bool(true)]
            ],
            "{sql}"
        );
    }
    let reached="SELECT EXISTS(SELECT CASE WHEN m.id=1 THEN EXISTS(SELECT i.n / i.n FROM lazy_ancestor i WHERE i.id=o.id) ELSE false END FROM lazy_ancestor m WHERE m.id=1) FROM lazy_ancestor o WHERE o.id=1";
    let error = fixture
        .cassie
        .execute_sql(&fixture.session, reached, vec![])
        .expect_err("reached ancestor error");
    assert!(error.to_string().contains("division by zero"), "{error}");
    let eager="SELECT EXISTS(SELECT CASE WHEN true THEN false ELSE EXISTS(SELECT i.n / i.n FROM lazy_ancestor i WHERE i.id=1) END FROM lazy_ancestor m WHERE m.id=1) FROM lazy_ancestor o WHERE o.id<0";
    let error = fixture
        .cassie
        .execute_sql(&fixture.session, eager, vec![])
        .expect_err("uncorrelated nested dead branch folds even on empty outer input");
    assert!(error.to_string().contains("division by zero"), "{error}");
}

#[test]
fn should_preserve_quoted_literal_ancestor_fields_in_nested_scope() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE literal_ancestor(id BIGINT,\"a.b\" BIGINT,\"A.B\" BIGINT)",
        "INSERT INTO literal_ancestor VALUES (1,1,2),(2,2,1)",
        "CREATE TABLE literal_member(id BIGINT)",
        "INSERT INTO literal_member VALUES (1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("literal ancestor setup");
    }
    let sql="SELECT o.id,EXISTS(SELECT 1 FROM literal_ancestor m WHERE m.id=1 AND EXISTS(SELECT 1 FROM literal_member i WHERE i.id=o.\"a.b\")) AS lower_match,EXISTS(SELECT 1 FROM literal_ancestor m WHERE m.id=1 AND EXISTS(SELECT 1 FROM literal_member i WHERE i.id=o.\"A.B\")) AS upper_match FROM literal_ancestor o ORDER BY o.id";
    // Act
    let result = fixture.cassie.execute_sql(&fixture.session, sql, vec![]);
    // Assert
    assert_eq!(
        result.expect("literal ancestor identity").rows,
        vec![
            vec![Value::Int64(1), Value::Bool(true), Value::Bool(false)],
            vec![Value::Int64(2), Value::Bool(false), Value::Bool(true)],
        ]
    );
}

#[test]
fn should_preserve_nested_scope_folding_boundaries() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE folding_outer(id BIGINT,n BIGINT)",
        "INSERT INTO folding_outer VALUES (1,0),(2,0)",
        "CREATE TABLE folding_middle(id BIGINT)",
        "INSERT INTO folding_middle VALUES (10)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("folding boundary setup");
    }
    let queries=[
        "SELECT o.id,EXISTS(SELECT 1 FROM folding_middle m WHERE EXISTS(WITH o AS (SELECT id FROM folding_outer WHERE id=1) SELECT 1 FROM o WHERE o.id=1)) AS matched FROM folding_outer o ORDER BY o.id",
        "SELECT o.id,EXISTS(SELECT 1 FROM folding_middle m WHERE EXISTS(SELECT 1 FROM (SELECT id FROM folding_outer WHERE id=1) o WHERE o.id=1)) AS matched FROM folding_outer o ORDER BY o.id",
        "SELECT o.id,EXISTS(SELECT CASE WHEN m.id=10 THEN false ELSE EXISTS(SELECT i.n / i.n FROM folding_outer i WHERE i.id=o.id) END FROM folding_middle m) AS matched FROM folding_outer o ORDER BY o.id",
    ];
    // Act
    let results = queries
        .iter()
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]))
        .collect::<Vec<_>>();
    let empty_correlated=fixture.cassie.execute_sql(&fixture.session,
        "SELECT EXISTS(SELECT EXISTS(SELECT i.n / i.n FROM folding_outer i WHERE i.id=o.id) FROM folding_middle m) AS matched FROM folding_outer o WHERE o.id<0",vec![]);
    let empty_uncorrelated=fixture.cassie.execute_sql(&fixture.session,
        "SELECT EXISTS(SELECT CASE WHEN true THEN false ELSE EXISTS(SELECT i.n / i.n FROM folding_outer i WHERE i.id=1) END FROM folding_middle m) AS matched FROM folding_outer o WHERE o.id<0",vec![]);
    // Assert
    for (sql, result) in queries.iter().zip(results) {
        assert_eq!(
            result.unwrap_or_else(|error| panic!("{sql}: {error}")).rows,
            vec![
                vec![Value::Int64(1), Value::Bool(true)],
                vec![Value::Int64(2), Value::Bool(true)]
            ],
            "{sql}"
        );
    }
    let empty = empty_correlated.expect("ancestor correlation defers beyond empty source");
    assert_eq!(empty.rows, Vec::<Vec<Value>>::new());
    assert_eq!(
        (
            empty.columns[0].name.as_str(),
            empty.columns[0].type_oid,
            empty.columns[0].typlen,
            empty.columns[0].atttypmod,
            empty.columns[0].format_code
        ),
        ("matched", 16, 1, -1, 0)
    );
    let error = empty_uncorrelated.expect_err("true uncorrelated occurrence still folds eagerly");
    assert!(
        matches!(error,cassie::app::CassieError::Execution(ref message) if message.contains("division by zero")),
        "{error}"
    );
}
