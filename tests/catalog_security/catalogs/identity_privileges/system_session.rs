//! Current-session activity, lock, and object-lookup visibility contracts for #817/#818.

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
        .expect("connect the current PostgreSQL wire session");
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
fn should_filter_object_lookup_by_schema_usage_privilege() {
    // Arrange
    let fixture = sql_fixture(
        "issue-818-restricted-role-object-lookup",
        &[
            "CREATE ROLE issue_818_lookup_reader",
            "CREATE SCHEMA issue_818_lookup_visibility",
            "CREATE TABLE issue_818_lookup_visibility.restricted_table (id INTEGER)",
            "CREATE FUNCTION issue_818_lookup_visibility.restricted_function(INTEGER) \
             RETURNS INTEGER LANGUAGE SQL IMMUTABLE AS $$ SELECT 1 $$",
            "REVOKE ALL ON SCHEMA issue_818_lookup_visibility FROM PUBLIC",
            "REVOKE ALL ON TABLE issue_818_lookup_visibility.restricted_table FROM PUBLIC",
            "REVOKE ALL ON FUNCTION \
             issue_818_lookup_visibility.restricted_function(INTEGER) FROM PUBLIC",
            "GRANT USAGE ON SCHEMA issue_818_lookup_visibility TO issue_818_lookup_reader",
        ],
    );
    fixture.cassie.startup().expect("startup");
    let reader = fixture
        .cassie
        .create_session("issue_818_lookup_reader", None);
    fixture
        .cassie
        .execute_sql(
            &reader,
            "SET search_path TO issue_818_lookup_visibility, public",
            vec![],
        )
        .expect("set the restricted role's search path");

    // Act
    let visible = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT to_regclass('restricted_table')::text, \
                    to_regprocedure('restricted_function(integer)')::text",
            vec![],
        )
        .expect("resolve names in a schema where the role has USAGE");
    fixture
        .execute(
            "REVOKE USAGE ON SCHEMA issue_818_lookup_visibility \
             FROM issue_818_lookup_reader",
        )
        .expect("remove schema visibility from the restricted role");
    let hidden = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT to_regclass('restricted_table') IS NULL, \
                    to_regprocedure('restricted_function(integer)') IS NULL",
            vec![],
        )
        .expect("hide names from a search path whose schema is not accessible");

    // Assert
    assert_eq!(
        visible.rows(),
        vec![vec![
            Value::String("restricted_table".to_owned()),
            Value::String("restricted_function(integer)".to_owned()),
        ]]
    );
    assert_eq!(
        hidden.rows(),
        vec![vec![Value::Bool(true), Value::Bool(true)]]
    );
}

include!("../relations/system_session.rs");

include!("../system_information/system_session.rs");
