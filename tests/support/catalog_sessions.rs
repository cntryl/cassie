//! Isolated pgwire sessions for PostgreSQL catalog concurrency contracts.

use std::sync::Arc;

use crate::support_pgwire::{self, PgwireServer};
use crate::support_sql;
use cassie::app::Cassie;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

pub struct Fixture {
    pub server: PgwireServer,
    path: String,
}

pub struct Session {
    pub client: Arc<Client>,
    connection: JoinHandle<Result<(), tokio_postgres::Error>>,
}

pub async fn start(label: &str) -> Fixture {
    support_sql::use_local_storage();
    let path = support_sql::data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create catalog fixture");
    cassie.startup().expect("start catalog fixture");
    let server = support_pgwire::spawn_server(cassie).await;
    Fixture { server, path }
}

pub async fn connect(fixture: &Fixture) -> Session {
    let password = std::env::var("CASSIE_ROOT_PASSWORD").unwrap_or_else(|_| "postgres".to_owned());
    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(fixture.server.addr.port())
        .user("root")
        .password(&password)
        .dbname("postgres");
    let (client, connection) = config
        .connect(NoTls)
        .await
        .expect("connect catalog session");
    Session {
        client: Arc::new(client),
        connection: tokio::spawn(connection),
    }
}

pub async fn stop(fixture: Fixture, sessions: Vec<Session>) {
    for session in sessions {
        drop(session.client);
        session.connection.abort();
        let _ = session.connection.await;
    }
    fixture.server.stop().await;
    std::fs::remove_dir_all(fixture.path).expect("remove isolated catalog fixture");
}
