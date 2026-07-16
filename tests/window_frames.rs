use cassie::app::{Cassie, CassieError};
use cassie::sql::ast::{
    QueryStatement, SelectItem, WindowFrameBound, WindowFrameExclusion, WindowFrameUnit,
};
use cassie::sql::parse_statement;
use cassie::types::Value;
use tokio_postgres::{Config, NoTls};

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

#[path = "support/pgwire.rs"]
mod wire;

fn execute_window_query(
    label: &str,
    query: &str,
) -> Result<cassie::executor::QueryResult, CassieError> {
    with_fallback();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE window_frame_values (category TEXT, ordinal INT, value INT)",
            vec![],
        )
        .expect("create window fixture");
    for (category, ordinal, value) in [
        ("a", 1, 10),
        ("a", 2, 20),
        ("a", 3, 20),
        ("a", 4, 30),
        ("b", 1, 99),
    ] {
        cassie
            .execute_sql(
                &session,
                &format!(
                    "INSERT INTO window_frame_values (category, ordinal, value) VALUES ('{category}', {ordinal}, {value})"
                ),
                vec![],
            )
            .expect("insert window fixture row");
    }
    let result = cassie.execute_sql(&session, query, vec![]);
    let _ = std::fs::remove_dir_all(path);
    result
}

fn values_for_column(result: &cassie::executor::QueryResult, index: usize) -> Vec<Value> {
    result.rows.iter().map(|row| row[index].clone()).collect()
}

#[test]
fn should_parse_explicit_rows_frame_bounds() {
    // Arrange
    let sql = "SELECT first_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) FROM window_frame_values";

    // Act
    let parsed = parse_statement(sql).expect("parse explicit ROWS frame");

    // Assert
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("expected SELECT");
    };
    let SelectItem::WindowFunction { function, .. } = &select.projection[0] else {
        panic!("expected window function");
    };
    assert_eq!(
        function.frame,
        Some(cassie::sql::ast::WindowFrame {
            unit: WindowFrameUnit::Rows,
            start: WindowFrameBound::Preceding(1),
            end: WindowFrameBound::CurrentRow,
            exclusion: WindowFrameExclusion::NoOthers,
        })
    );
}

#[test]
fn should_apply_ordered_default_rows_frame() {
    // Arrange
    let query = "SELECT ordinal, first_value(value) OVER (PARTITION BY category ORDER BY ordinal) AS first_value, last_value(value) OVER (PARTITION BY category ORDER BY ordinal) AS last_value FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

    // Act
    let result = execute_window_query("window_default_rows", query).expect("execute default frame");

    // Assert
    assert_eq!(
        result.rows,
        vec![
            vec![Value::Int64(1), Value::Int64(10), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(10), Value::Int64(20)],
            vec![Value::Int64(3), Value::Int64(10), Value::Int64(20)],
            vec![Value::Int64(4), Value::Int64(10), Value::Int64(30)],
        ]
    );
}

#[test]
fn should_apply_whole_partition_rows_frame() {
    // Arrange
    let query = "SELECT ordinal, first_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING), last_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

    // Act
    let result =
        execute_window_query("window_whole_partition", query).expect("execute whole frame");

    // Assert
    assert_eq!(
        result.rows,
        vec![
            vec![Value::Int64(1), Value::Int64(10), Value::Int64(30)],
            vec![Value::Int64(2), Value::Int64(10), Value::Int64(30)],
            vec![Value::Int64(3), Value::Int64(10), Value::Int64(30)],
            vec![Value::Int64(4), Value::Int64(10), Value::Int64(30)],
        ]
    );
}

#[test]
fn should_apply_bounded_preceding_following_rows_frame() {
    // Arrange
    let query = "SELECT ordinal, first_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING), last_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

    // Act
    let result = execute_window_query("window_bounded_rows", query).expect("execute bounded frame");

    // Assert
    assert_eq!(
        result.rows,
        vec![
            vec![Value::Int64(1), Value::Int64(10), Value::Int64(20)],
            vec![Value::Int64(2), Value::Int64(10), Value::Int64(20)],
            vec![Value::Int64(3), Value::Int64(20), Value::Int64(30)],
            vec![Value::Int64(4), Value::Int64(20), Value::Int64(30)],
        ]
    );
}

#[test]
fn should_keep_frame_independent_functions() {
    // Arrange
    let query = "SELECT ordinal, rank() OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN CURRENT ROW AND CURRENT ROW), lag(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN CURRENT ROW AND CURRENT ROW), lead(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN CURRENT ROW AND CURRENT ROW) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

    // Act
    let result = execute_window_query("window_frame_independent", query)
        .expect("execute frame-independent functions");

    // Assert
    assert_eq!(
        result.rows,
        vec![
            vec![
                Value::Int64(1),
                Value::Int64(1),
                Value::Null,
                Value::Int64(20)
            ],
            vec![
                Value::Int64(2),
                Value::Int64(2),
                Value::Int64(10),
                Value::Int64(20)
            ],
            vec![
                Value::Int64(3),
                Value::Int64(3),
                Value::Int64(20),
                Value::Int64(30)
            ],
            vec![
                Value::Int64(4),
                Value::Int64(4),
                Value::Int64(20),
                Value::Null
            ],
        ]
    );
}

#[test]
fn should_apply_rows_frame_without_collapsing_peers() {
    // Arrange
    let query = "SELECT ordinal, last_value(value) OVER (PARTITION BY category ORDER BY value ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

    // Act
    let result = execute_window_query("window_rows_peers", query).expect("execute peer frame");

    // Assert
    assert_eq!(
        values_for_column(&result, 1),
        vec![
            Value::Int64(10),
            Value::Int64(20),
            Value::Int64(20),
            Value::Int64(30),
        ]
    );
}

#[test]
fn should_handle_empty_window_partition() {
    // Arrange
    let empty_query = "SELECT first_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) FROM window_frame_values WHERE category = 'missing'";
    let single_query = "SELECT first_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING), last_value(value) OVER (PARTITION BY category ORDER BY ordinal ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING) FROM window_frame_values WHERE category = 'b'";

    // Act
    let empty =
        execute_window_query("window_empty_partition", empty_query).expect("empty partition");
    let single =
        execute_window_query("window_single_partition", single_query).expect("single partition");

    // Assert
    assert!(empty.rows.is_empty());
    assert_eq!(single.rows, vec![vec![Value::Int64(99), Value::Int64(99)]]);
}

#[test]
fn should_apply_range_window_frame_to_peers() {
    // Arrange
    let query = "SELECT ordinal, first_value(value) OVER (ORDER BY value RANGE BETWEEN CURRENT ROW AND CURRENT ROW), last_value(value) OVER (ORDER BY value RANGE BETWEEN CURRENT ROW AND CURRENT ROW) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

    // Act
    let result = execute_window_query("window_range_peers", query).expect("execute RANGE frame");

    // Assert
    assert_eq!(
        result.rows,
        vec![
            vec![Value::Int64(1), Value::Int64(10), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(20), Value::Int64(20)],
            vec![Value::Int64(3), Value::Int64(20), Value::Int64(20)],
            vec![Value::Int64(4), Value::Int64(30), Value::Int64(30)],
        ]
    );
}

#[test]
fn should_keep_large_integer_range_offsets_exact() {
    // Arrange
    with_fallback();
    let path = data_dir("window_range_exact_integer");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE exact_range_values (ordinal BIGINT)",
            vec![],
        )
        .expect("create table");
    for value in [9_007_199_254_740_992_i64, 9_007_199_254_740_993_i64] {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO exact_range_values (ordinal) VALUES ($1)",
                vec![Value::Int64(value)],
            )
            .expect("insert exact integer");
    }

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT ordinal, last_value(ordinal) OVER (ORDER BY ordinal RANGE BETWEEN 0 PRECEDING AND 0 FOLLOWING) FROM exact_range_values ORDER BY ordinal",
            vec![],
        )
        .expect("execute exact RANGE frame");

    // Assert
    assert_eq!(
        result.rows,
        vec![
            vec![
                Value::Int64(9_007_199_254_740_992),
                Value::Int64(9_007_199_254_740_992),
            ],
            vec![
                Value::Int64(9_007_199_254_740_993),
                Value::Int64(9_007_199_254_740_993),
            ],
        ]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_apply_groups_window_frame_offsets() {
    // Arrange
    let query = "SELECT ordinal, first_value(value) OVER (ORDER BY value GROUPS BETWEEN 1 PRECEDING AND CURRENT ROW), last_value(value) OVER (ORDER BY value GROUPS BETWEEN CURRENT ROW AND 1 FOLLOWING) FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

    // Act
    let result =
        execute_window_query("window_groups_offsets", query).expect("execute GROUPS frame");

    // Assert
    assert_eq!(
        result.rows,
        vec![
            vec![Value::Int64(1), Value::Int64(10), Value::Int64(20)],
            vec![Value::Int64(2), Value::Int64(10), Value::Int64(30)],
            vec![Value::Int64(3), Value::Int64(10), Value::Int64(30)],
            vec![Value::Int64(4), Value::Int64(20), Value::Int64(30)],
        ]
    );
}

#[test]
fn should_apply_window_frame_exclusions() {
    // Arrange
    let query = "SELECT ordinal, last_value(value) OVER (ORDER BY value ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE CURRENT ROW) AS without_current, first_value(value) OVER (ORDER BY value ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE GROUP) AS without_group, first_value(value) OVER (ORDER BY value ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE TIES) AS without_ties FROM window_frame_values WHERE category = 'a' ORDER BY ordinal";

    // Act
    let result = execute_window_query("window_exclusions", query).expect("execute exclusions");

    // Assert
    assert_eq!(
        result.rows,
        vec![
            vec![
                Value::Int64(1),
                Value::Int64(30),
                Value::Int64(20),
                Value::Int64(10)
            ],
            vec![
                Value::Int64(2),
                Value::Int64(30),
                Value::Int64(10),
                Value::Int64(10)
            ],
            vec![
                Value::Int64(3),
                Value::Int64(30),
                Value::Int64(10),
                Value::Int64(10)
            ],
            vec![
                Value::Int64(4),
                Value::Int64(20),
                Value::Int64(10),
                Value::Int64(10)
            ],
        ]
    );
}

#[test]
fn should_reject_invalid_window_frame_order() {
    // Arrange
    let query = "SELECT first_value(value) OVER (ORDER BY ordinal ROWS BETWEEN CURRENT ROW AND 1 PRECEDING) FROM window_frame_values";

    // Act
    let error = execute_window_query("window_invalid_order", query)
        .expect_err("invalid frame order should be rejected");

    // Assert
    assert!(matches!(error, CassieError::Unsupported(message) if message.contains("frame bounds")));
}

#[test]
fn should_reject_negative_window_frame_offset() {
    // Arrange
    let query = "SELECT first_value(value) OVER (ORDER BY ordinal ROWS BETWEEN -1 PRECEDING AND CURRENT ROW) FROM window_frame_values";

    // Act
    let error = execute_window_query("window_negative_offset", query)
        .expect_err("negative frame offset should be rejected");

    // Assert
    assert!(matches!(error, CassieError::Unsupported(message) if message.contains("negative")));
}

#[test]
fn should_default_deserialized_window_frame_exclusion() {
    // Arrange
    let serialized = r#"{"unit":"Rows","start":"UnboundedPreceding","end":"CurrentRow"}"#;

    // Act
    let frame: cassie::sql::ast::WindowFrame =
        serde_json::from_str(serialized).expect("deserialize legacy frame");

    // Assert
    assert_eq!(frame.exclusion, WindowFrameExclusion::NoOthers);
}

#[test]
fn should_not_return_unsupported_window_frame_sqlstate() {
    // Arrange
    with_fallback();
    let path = data_dir("window_pgwire_error");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let server = wire::spawn_server(cassie).await;
        let mut config = Config::new();
        config.host("127.0.0.1");
        config.port(server.addr.port());
        config.user("postgres");
        config.dbname("postgres");
        config.password("postgres");
        let (client, connection) = config.connect(NoTls).await.expect("connect pgwire");
        let connection = tokio::spawn(async move {
            let _ = connection.await;
        });

        // Act
        let error = client
            .query(
                "SELECT first_value(value) OVER (ORDER BY ordinal RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM missing_window_frame_values",
                &[],
            )
            .await
            .expect_err("missing relation should be reported");

        // Assert
        assert_ne!(
            error
                .as_db_error()
                .expect("database error")
                .code()
                .code(),
            "0A000"
        );

        drop(client);
        connection.abort();
        let _ = connection.await;
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}
