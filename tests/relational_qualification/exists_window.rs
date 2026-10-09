use crate::support_relational_qualification::fixture;
use cassie::types::Value;

#[test]
fn should_resolve_window_exists_against_selected_frame_input() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE window_outer(id BIGINT)",
        "CREATE TABLE window_inner(id BIGINT)",
        "INSERT INTO window_outer VALUES (1),(2)",
        "INSERT INTO window_inner VALUES (1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("window setup");
    }
    let queries=[
        "SELECT id,LAST_VALUE(EXISTS(SELECT 1 FROM window_inner u WHERE u.id=q.id)) OVER (ORDER BY id) AS matched FROM window_outer q ORDER BY id",
        "SELECT id,LAST_VALUE(EXISTS(SELECT 1 FROM window_inner u WHERE u.id=q.id)) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) AS matched FROM window_outer q ORDER BY id",
        "SELECT id,LAG(EXISTS(SELECT 1 FROM window_inner u WHERE u.id=q.id)) OVER (ORDER BY id) AS matched FROM window_outer q ORDER BY id",
    ];
    // Act
    let results = queries
        .iter()
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]))
        .collect::<Vec<_>>();
    // Assert
    for ((sql, result), index) in queries.iter().zip(results).zip(0..) {
        let result = result.unwrap_or_else(|error| panic!("{sql}: {error}"));
        let values = match index {
            0 => [Value::Bool(true), Value::Bool(false)],
            1 => [Value::Bool(false), Value::Bool(false)],
            _ => [Value::Null, Value::Bool(true)],
        };
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(1), values[0].clone()],
                vec![Value::Int64(2), values[1].clone()]
            ],
            "{sql}"
        );
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
