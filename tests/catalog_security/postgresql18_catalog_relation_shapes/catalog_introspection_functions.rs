//! PostgreSQL 18.6 session information function contracts for #817.

use crate::support_pgwire::{self, PgwireServer};
use crate::support_sql;
use crate::support_sql_fixture::sql_fixture;
use cassie::app::Cassie;
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
    support_sql::use_local_storage();
    let path = support_sql::data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
    cassie.startup().expect("start Cassie instance");
    let server = support_pgwire::spawn_server(cassie).await;

    let password = std::env::var("CASSIE_ROOT_PASSWORD").unwrap_or_else(|_| "postgres".to_owned());
    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(server.addr.port())
        .user("root")
        .password(&password)
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
    std::fs::remove_dir_all(path).expect("remove the isolated pgwire data directory");
}

#[test]
fn should_report_current_catalog_as_the_active_database() {
    // Arrange
    let fixture = sql_fixture("issue-817-current-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT current_catalog, current_database(), \
                    current_catalog = current_database()",
        )
        .expect("read the current catalog name and compare it with the active database");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![19, 19, 16]
    );
    assert_eq!(
        selected.rows(),
        &[vec![
            Value::String("postgres".to_owned()),
            Value::String("postgres".to_owned()),
            Value::Bool(true),
        ]]
    );
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

#[test]
fn should_report_the_negotiated_client_encoding_over_pgwire() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (server, client, connection, path) =
            start_pgwire_fixture("issue-817-client-encoding").await;

        // Act
        let row = client
            .query_one(
                "SELECT pg_catalog.pg_client_encoding(), \
                        current_setting('client_encoding')",
                &[],
            )
            .await
            .expect("read the negotiated client encoding and session setting");

        // Assert
        assert_eq!(
            row.columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![19, 25]
        );
        assert_eq!(
            (row.get::<_, String>(0), row.get::<_, String>(1),),
            ("UTF8".to_owned(), "UTF8".to_owned())
        );

        stop_pgwire_fixture(server, client, connection, path).await;
    });
}
