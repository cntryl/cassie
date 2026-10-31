//! PostgreSQL 18.6 session information function contracts for #817.

use crate::support_pgwire::{self, PgwireServer};
use crate::support_sql;
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;
use tokio_postgres::{Client, NoTls};

async fn start_pgwire_fixture(
    label: &str,
) -> (
    PgwireServer,
    Client,
    tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
    String,
) {
    let path = support_sql::data_dir(label);
    let cassie = support_sql::new_in_memory_cassie().expect("create Cassie instance");
    cassie.startup().expect("start Cassie instance");
    let server = support_pgwire::spawn_server(cassie).await;

    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(server.addr.port())
        .user("root")
        .password(crate::support_sql::test_root_password())
        .dbname("postgres");
    let (client, connection) = config
        .connect(NoTls)
        .await
        .expect("connect the PostgreSQL wire session");
    let connection = tokio::spawn(connection);

    (server, client, connection, path)
}

async fn stop_pgwire_fixture(
    server: PgwireServer,
    client: Client,
    connection: tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
    path: String,
) {
    drop(client);
    connection.abort();
    let _ = connection.await;
    server.stop().await;
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_render_abs_integer_catalog_signatures() {
    // Arrange
    let fixture = sql_fixture("issue-817-function-signature-introspection", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_get_function_arguments(1397::pg_catalog.oid), \
                    pg_catalog.pg_get_function_identity_arguments(1397::pg_catalog.oid), \
                    pg_catalog.pg_get_function_result(1397::pg_catalog.oid)",
        )
        .expect("render PostgreSQL's argument, identity, and result signatures for abs(integer)");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25, 25]
    );
    assert_eq!(
        selected.rows(),
        &[vec![
            Value::String("integer".to_owned()),
            Value::String("integer".to_owned()),
            Value::String("integer".to_owned()),
        ]]
    );
}

include!("../system_information/catalog_introspection_functions.rs");
