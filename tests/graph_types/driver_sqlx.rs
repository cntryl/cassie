//! The `sqlx` type-resolver boundary of the reserved graph types (#1268).
//!
//! `sqlx` 0.9 resolves user-defined composite and array OIDs with a catalog query that needs
//! `pg_catalog.pg_range`, an oid-typed `pg_attribute.attrelid`, `regtype` casts, row values in
//! `array_agg` and alias-without-`AS` after function calls. Cassie has none of those yet (the
//! catalog scope is #905), so the `sqlx` round-trip of the graph types is a recorded blocker and
//! this module pins only the boundary. The first real driver is `tokio-postgres`
//! (`driver.rs`). The connection is plaintext to a local listener owned by the test; the
//! runtime is a current-thread runtime built by hand.

use crate::driver::{run, serve, Served};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{raw_sql, ConnectOptions, Connection, PgConnection};

struct Server {
    connection: PgConnection,
    accept_loop: tokio::task::JoinHandle<()>,
}

/// The SQLSTATE of a failed step, or `none` when the failure is not a database error.
fn state(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map_or_else(|| "none".to_owned(), |code| code.into_owned())
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
