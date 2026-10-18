//! Current-session activity, lock, and object-lookup visibility contracts for #817/#818.

use crate::support_pgwire::{self, PgwireServer};
use crate::support_sql;
use cassie::app::Cassie;
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

include!("../relations/system_session.rs");

include!("../system_information/system_session.rs");
