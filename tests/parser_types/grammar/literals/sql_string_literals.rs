use super::support_sql as support;

use cassie::app::Cassie;
use cassie::sql::ast::{Expr, QueryStatement, SelectItem};
use cassie::sql::parse_statement;
use cassie::types::Value;

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
fn should_evaluate_concatenation_given_literals_and_columns_when_used_in_queries_and_dml() {
    // Arrange
    let fixture = crate::support_sql_fixture::sql_fixture("concatenation-contexts", &[]);
    let cassie = &fixture.cassie;
    let session = &fixture.session;
    for sql in [
        "CREATE TABLE concat_names (k INT, name TEXT)",
        "INSERT INTO concat_names VALUES (1, 'alpha')",
    ] {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("seed concatenation fixture");
    }
    let cases = [
        ("SELECT 'a' || 'b'", "ab"),
        ("SELECT 'a'||'b'", "ab"),
        ("SELECT '(' || name || ')' FROM concat_names", "(alpha)"),
        ("SELECT '[' || trim('  x  ') || ']'", "[x]"),
        ("SELECT name || 'x' FROM concat_names", "alphax"),
        (
            "SELECT name FROM concat_names WHERE name = 'al' || 'pha'",
            "alpha",
        ),
    ];

    // Act
    let selected = cases.map(|(sql, expected)| {
        (
            cassie
                .execute_sql(session, sql, vec![])
                .expect("evaluate concatenation"),
            expected,
        )
    });
    cassie
        .execute_sql(
            session,
            "INSERT INTO concat_names VALUES (2, 'a' || 'b')",
            vec![],
        )
        .expect("concatenation in INSERT");
    cassie
        .execute_sql(
            session,
            "UPDATE concat_names SET name = 'c' || 'd' WHERE k = 2",
            vec![],
        )
        .expect("concatenation in UPDATE");
    cassie
        .execute_sql(
            session,
            "DELETE FROM concat_names WHERE 'x' = 'x' || 'y'",
            vec![],
        )
        .expect("concatenation in DELETE predicate");
    let remaining = cassie
        .execute_sql(session, "SELECT name FROM concat_names ORDER BY k", vec![])
        .expect("read mutation effects");

    // Assert
    for (result, expected) in selected {
        assert_eq!(
            result.rows(),
            vec![vec![Value::String(expected.to_string())]]
        );
    }
    assert_eq!(
        remaining.rows(),
        vec![
            vec![Value::String("alpha".to_string())],
            vec![Value::String("cd".to_string())]
        ]
    );
}

#[test]
fn should_apply_escape_characters_given_literal_wildcards_when_matching_like_patterns() {
    // Arrange
    let fixture = crate::support_sql_fixture::sql_fixture("like-escape-values", &[]);
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE '!', 'axb' LIKE 'a!_b' ESCAPE '!', 'a!xb' LIKE 'a!!_b' ESCAPE '!', 'a_b' LIKE 'a!_b' ESCAPE ''";

    // Act
    let selected = fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![])
        .expect("execute LIKE ESCAPE");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false)
        ]]
    );
}

#[test]
fn should_reject_adjacent_string_literals_as_one_token() {
    // Arrange
    let sql = "SELECT 'a' 'b'";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err(), "adjacent literals must not collapse");
}

#[test]
fn should_keep_operator_text_inside_a_single_string_literal() {
    // Arrange
    let cases = [
        ("SELECT 'a || b'", "a || b"),
        ("SELECT 'a'' || ''b'", "a' || 'b"),
        ("SELECT 'x LIKE y ESCAPE z'", "x LIKE y ESCAPE z"),
        ("SELECT ''", ""),
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
                selected.rows(),
                vec![
                    vec![Value::String("O'Brien".to_string())],
                    vec![Value::String("'".to_string())],
                    vec![Value::String("one'two'three".to_string())],
                ]
            );
            let _ = std::fs::remove_dir_all(path);
        });
}
