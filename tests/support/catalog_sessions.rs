//! Isolated pgwire sessions for PostgreSQL catalog concurrency contracts.

use std::sync::Arc;

use crate::support_pgwire::{self, PgwireServer};
use crate::support_sql;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

pub struct Fixture {
    pub server: PgwireServer,
}

pub struct Session {
    pub client: Arc<Client>,
    connection: JoinHandle<Result<(), tokio_postgres::Error>>,
}

pub async fn start(_label: &str) -> Fixture {
    let cassie = support_sql::new_in_memory_cassie().expect("create catalog fixture");
    cassie.startup().expect("start catalog fixture");
    let server = support_pgwire::spawn_server(cassie).await;
    Fixture { server }
}

pub async fn connect(fixture: &Fixture) -> Session {
    let password = std::env::var("CASSIE_ROOT_PASSWORD").unwrap_or_else(|_| crate::support_sql::test_root_password().to_owned());
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
}
