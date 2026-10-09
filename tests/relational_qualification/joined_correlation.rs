use crate::support_relational_qualification::fixture;
use cassie::types::Value;

#[test]
fn should_preserve_joined_delimited_outer_fields_in_where_exists() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE joined_dotted (id BIGINT,\"a.b\" BIGINT)",
        "INSERT INTO joined_dotted (id,\"a.b\") VALUES (1,10),(2,20)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("joined literal-field setup");
    }
    let queries = [
        "SELECT r.\"a.b\" FROM joined_dotted l JOIN joined_dotted r ON l.id=r.id ORDER BY l.id",
        "SELECT EXISTS(SELECT 1 FROM joined_dotted u WHERE u.\"a.b\"=r.\"a.b\") FROM joined_dotted l JOIN joined_dotted r ON l.id=r.id ORDER BY l.id",
        "SELECT l.id FROM joined_dotted l JOIN joined_dotted r ON l.id=r.id WHERE EXISTS(SELECT 1 FROM joined_dotted u WHERE u.\"a.b\"=r.\"a.b\") ORDER BY l.id",
    ];

    // Act
    let results = queries
        .iter()
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]))
        .collect::<Vec<_>>();

    // Assert
    let expected = [
        vec![vec![Value::Int64(10)], vec![Value::Int64(20)]],
        vec![vec![Value::Bool(true)], vec![Value::Bool(true)]],
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]],
    ];
    for ((sql, result), expected) in queries.iter().zip(results).zip(expected) {
        let result = result.unwrap_or_else(|error| panic!("{sql}: {error}"));
        assert_eq!(result.rows, expected, "{sql}");
    }
}

#[test]
fn should_distinguish_joined_case_specific_literal_fields() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE joined_case (id BIGINT,\"a.b\" BIGINT,\"A.B\" BIGINT)",
        "INSERT INTO joined_case (id,\"a.b\",\"A.B\") VALUES (1,10,20),(2,20,10)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("case-distinct joined setup");
    }
    let queries = [
        "SELECT r.id FROM joined_case l JOIN joined_case r ON l.id=r.id WHERE EXISTS(SELECT 1 FROM joined_case u WHERE u.id=1 AND u.\"a.b\"=r.\"a.b\") ORDER BY r.id",
        "SELECT r.id FROM joined_case l JOIN joined_case r ON l.id=r.id WHERE EXISTS(SELECT 1 FROM joined_case u WHERE u.id=1 AND u.\"a.b\"=r.\"A.B\") ORDER BY r.id",
    ];

    // Act
    let results = queries
        .iter()
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, result), id) in queries.iter().zip(results).zip([1, 2]) {
        let result = result.unwrap_or_else(|error| panic!("{sql}: {error}"));
        assert_eq!(result.rows, vec![vec![Value::Int64(id)]], "{sql}");
    }
}

#[test]
fn should_prefer_inner_literal_field_over_joined_outer_fields() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE joined_shadow (id BIGINT,\"a.b\" BIGINT)",
        "INSERT INTO joined_shadow (id,\"a.b\") VALUES (1,10),(2,20)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("inner-shadow joined setup");
    }
    let sql = "SELECT r.id FROM joined_shadow l JOIN joined_shadow r ON l.id=r.id WHERE EXISTS(SELECT 1 FROM joined_shadow u WHERE u.id=1 AND \"a.b\"=10 AND r.id>0) ORDER BY r.id";

    // Act
    let result = fixture.cassie.execute_sql(&fixture.session, sql, vec![]);

    // Assert
    assert_eq!(
        result.expect("inner literal namespace").rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}
