use cassie::app::Cassie;
use cassie::sql::ast::{Expr, QueryStatement, SelectItem};
use cassie::sql::parse_statement;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, use_local_storage};

fn parse_projected_string(sql: &str) -> String {
    let parsed = parse_statement(sql).expect("parse string literal projection");
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected SELECT statement");
    };
    let SelectItem::Expr {
        expr: Expr::StringLiteral(value),
        ..
    } = &statement.projection[0]
    else {
        panic!("expected projected string literal");
    };
    value.clone()
}

#[test]
fn should_unescape_every_doubled_quote_in_sql_string_literals() {
    // Arrange
    let cases = [
        ("SELECT 'O''Brien'", "O'Brien"),
        ("SELECT ''''", "'"),
        ("SELECT 'one''two''three'", "one'two'three"),
    ];

    // Act
    let parsed = cases.map(|(sql, expected)| (parse_projected_string(sql), expected));

    // Assert
    for (actual, expected) in parsed {
        assert_eq!(actual, expected);
    }
}

#[test]
fn should_round_trip_doubled_quotes_through_sql() {
    // Arrange
    use_local_storage();
    let path = data_dir("sql_string_literal_quotes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_string_literal_quotes (position BIGINT, value TEXT)",
                vec![],
            )
            .expect("create table");

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_string_literal_quotes (position, value) VALUES (1, 'O''Brien'), (2, ''''), (3, 'one''two''three')",
                vec![],
            )
            .expect("insert escaped literals");
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT value FROM sql_string_literal_quotes ORDER BY position",
                vec![],
            )
            .expect("select escaped literals");

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("O'Brien".to_string())],
                vec![Value::String("'".to_string())],
                vec![Value::String("one'two'three".to_string())],
            ]
        );
        let _ = std::fs::remove_dir_all(path);
    });
}
