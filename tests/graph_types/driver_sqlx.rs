//! The `sqlx` type-resolver boundary of the reserved graph types (#1268).
//!
//! `sqlx` 0.9 resolves user-defined composite and array OIDs with a catalog query that needs
//! `pg_catalog.pg_range`, an oid-typed `pg_attribute.attrelid`, `regtype` casts, row values in
//! `array_agg` and alias-without-`AS` after function calls. Cassie has none of those yet (the
//! catalog scope is #905), so the `sqlx` round-trip of the graph types is a recorded blocker and
//! this module pins the boundary and qualifies the builtin-typed results (no type resolution). The first real driver is `tokio-postgres`
//! (`driver.rs`). The connection is plaintext to a local listener owned by the test; the
//! runtime is a current-thread runtime built by hand.

use crate::driver::{run, serve, Served};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{raw_sql, ConnectOptions, Connection, PgConnection, Row};

struct Server {
    connection: PgConnection,
    accept_loop: tokio::task::JoinHandle<()>,
}

/// The SQLSTATE of a failed step, or `none` when the failure is not a database error.
fn state(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .map_or_else(|| "none".to_owned(), std::borrow::Cow::into_owned)
}

/// The value of a step that must succeed; a failure reports only its step and SQLSTATE.
fn must<T>(outcome: Result<T, sqlx::Error>, step: &str) -> T {
    match outcome {
        Ok(value) => value,
        Err(error) => panic!("{step} failed with SQLSTATE {}", state(&error)),
    }
}

async fn start() -> Server {
    let Served { port, accept_loop } = serve().await;
    let options = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username("root")
        .password(crate::support_sql::test_root_password())
        .database("postgres")
        .ssl_mode(PgSslMode::Disable);
    let connection = must(options.connect().await, "connect");
    Server {
        connection,
        accept_loop,
    }
}

impl Server {
    async fn stop(self) {
        let _ = self.connection.close().await;
        self.accept_loop.abort();
        let _ = self.accept_loop.await;
    }
}

#[test]
fn should_reject_the_sqlx_type_resolver_range_catalog_join_until_ranges_exist() {
    // Arrange
    let work = async {
        let mut server = start().await;
        let outcome = raw_sql(
            "SELECT t.oid, r.rngsubtype FROM pg_catalog.pg_type t \
             LEFT JOIN pg_catalog.pg_range r ON t.oid = r.rngtypid",
        )
        .fetch_all(&mut server.connection)
        .await;
        let sql_state = outcome.err().map(|error| state(&error));
        server.stop().await;
        sql_state
    };

    // Act
    let sql_state = run(work);

    // Assert
    assert_eq!(sql_state.as_deref(), Some("42P01"));
}

fn path() -> String {
    format!(
        "ROW(ARRAY[{}, {}], ARRAY[{}], ARRAY[1::smallint])::cassie_graph.graph_path",
        crate::driver::ROOM_1,
        crate::driver::ROOM_2,
        crate::driver::DOOR
    )
}

/// Runs `sql` through the extended protocol (binary results) and returns its rows.
async fn rows(server: &mut Server, sql: String) -> Vec<sqlx::postgres::PgRow> {
    must(
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut server.connection)
            .await,
        "query",
    )
}

#[test]
fn should_read_the_ordered_steps_of_a_path_as_scalars_through_sqlx() {
    // Arrange
    let work = async {
        let mut server = start().await;
        let sql = format!(
            "SELECT s.position, s.direction FROM cassie_graph.path_steps({}) AS s ORDER BY s.position",
            path()
        );

        let rows = rows(&mut server, sql).await;
        let steps: Vec<(i64, Option<i16>)> = rows
            .iter()
            .map(|row| (row.get::<i64, _>(0), row.get::<Option<i16>, _>(1)))
            .collect();
        server.stop().await;
        steps
    };

    // Act
    let steps = run(work);

    // Assert
    assert_eq!(steps, vec![(0, None), (1, Some(1))]);
}

#[test]
fn should_read_the_typed_key_of_an_element_through_sqlx() {
    // Arrange
    let work = async {
        let mut server = start().await;
        let sql = format!(
            "SELECT k.ordinal, k.type_tag, k.text_value, k.integer_value \
             FROM cassie_graph.key_parts({}) AS k ORDER BY k.ordinal",
            crate::driver::ROOM_1
        );

        let rows = rows(&mut server, sql).await;
        let parts: Vec<(i64, i16, Option<String>, Option<i32>)> = rows
            .iter()
            .map(|row| {
                (
                    row.get::<i64, _>(0),
                    row.get::<i16, _>(1),
                    row.get::<Option<String>, _>(2),
                    row.get::<Option<i64>, _>(3)
                        .map(|value| i32::try_from(value).expect("integer key")),
                )
            })
            .collect();
        server.stop().await;
        parts
    };

    // Act
    let parts = run(work);

    // Assert
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].0, 1);
    assert_eq!(parts[0].2.as_deref(), Some("t1"));
    assert_eq!(parts[1].0, 2);
    assert_eq!(parts[1].3, Some(1));
}

#[test]
fn should_export_the_versioned_jsonb_document_of_a_path_through_sqlx() {
    // Arrange
    let work = async {
        let mut server = start().await;
        let sql = format!("SELECT cassie_graph.path_to_jsonb({})::text", path());

        let rows = rows(&mut server, sql).await;
        let document = rows[0].get::<String, _>(0);
        server.stop().await;
        document
    };

    // Act
    let document = run(work);

    // Assert
    assert!(document.contains("\"version\": 1"));
    assert!(document.contains("\"directions\": [1]"));
    assert!(document.contains("\"kind\": \"edge\""));
}

#[test]
fn should_keep_a_bigint_key_exact_through_a_sqlx_bound_parameter() {
    // Arrange
    let work = async {
        let mut server = start().await;

        let row = must(
            sqlx::query("SELECT (cassie_graph.key_part($1::bigint)).integer_value")
                .bind(9_007_199_254_740_993_i64)
                .fetch_one(&mut server.connection)
                .await,
            "query",
        );
        let value = row.get::<i64, _>(0);
        server.stop().await;
        value
    };

    // Act
    let value = run(work);

    // Assert
    assert_eq!(value, 9_007_199_254_740_993);
}

#[test]
fn should_render_a_whole_path_in_composite_text_form_through_sqlx() {
    // Arrange
    let work = async {
        let mut server = start().await;
        let sql = format!("SELECT ({})::text", path());

        let rows = rows(&mut server, sql).await;
        let text = rows[0].get::<String, _>(0);
        server.stop().await;
        text
    };

    // Act
    let text = run(work);

    // Assert
    assert!(text.starts_with("(\"{\"\"("));
    assert!(text.ends_with(",{1})"));
    assert!(text.contains("(4,,t1,)"));
    assert!(text.contains("(5,,,00000000-0000-0000-0000-0000000000aa)"));
}
