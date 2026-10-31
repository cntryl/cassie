//! PostgreSQL 18.6 mutation and data-modifying CTE gaps for #823, #825 and #834.

use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

use super::support_sql_fixture::sql_fixture;

#[path = "../merge/merge_cardinality_atomicity/restart.rs"]
mod merge_cardinality_atomicity;
#[path = "../conflicts/returning_error_order/semantics.rs"]
mod returning_error_order;
#[path = "../sequences/truncate_restart_identity_commit/restart.rs"]
mod truncate_restart_identity_commit;
#[path = "../returning/update_returning_descriptor/semantics.rs"]
mod update_returning_descriptor;

struct PgwireFixture {
    client: Option<Client>,
    connection: Option<JoinHandle<()>>,
    server: Option<JoinHandle<Result<(), CassieError>>>,
    data_dir: String,
}

impl PgwireFixture {
    async fn start(label: &str) -> Self {
        let data_dir = super::support_sql::data_dir(label);
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        cassie.startup().expect("start Cassie");

        let config = crate::support_sql::with_test_root_password(
            CassieRuntimeConfig::from_env().expect("runtime config"),
        );
        let password = config.password.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire listener");
        let address: SocketAddr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            address.to_string(),
            Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client_config = tokio_postgres::Config::new();
        client_config
            .host("127.0.0.1")
            .port(address.port())
            .user("root")
            .password(&password)
            .dbname("postgres");
        let (client, connection) = client_config
            .connect(NoTls)
            .await
            .expect("connect pgwire client");
        let connection = tokio::spawn(async move {
            let _ = connection.await;
        });

        Self {
            client: Some(client),
            connection: Some(connection),
            server: Some(server),
            data_dir,
        }
    }

    fn client(&self) -> &Client {
        self.client.as_ref().expect("live pgwire client")
    }

    async fn shutdown(mut self) {
        drop(self.client.take());
        if let Some(connection) = self.connection.take() {
            connection.abort();
            let _ = connection.await;
        }
        if let Some(server) = self.server.take() {
            server.abort();
            let _ = server.await;
        }
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

impl Drop for PgwireFixture {
    fn drop(&mut self) {
        drop(self.client.take());
        if let Some(connection) = self.connection.take() {
            connection.abort();
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

fn db_error(error: &tokio_postgres::Error) -> &tokio_postgres::error::DbError {
    error.as_db_error().expect("PostgreSQL database error")
}

include!("mutation/errors.rs");

include!("../transactions/mutation/semantics.rs");

include!("../conflicts/mutation/transactions.rs");
