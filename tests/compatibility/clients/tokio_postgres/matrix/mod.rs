use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use std::{fs, path::PathBuf};

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use tokio_postgres::{error::DbError, NoTls, SimpleQueryMessage};
use uuid::Uuid;

#[path = "../../prisma/generated_queries.rs"]
mod prisma7_generated_query;

fn temp_dir(label: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-compatibility-{}-{}", label, Uuid::new_v4()));
    path
}

pub(super) struct CompatibilityServer {
    pub(super) addr: SocketAddr,
    pub(super) cassie: Cassie,
    server: tokio::task::JoinHandle<()>,
}

impl CompatibilityServer {
    pub(super) async fn start(_label: &str) -> Self {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_in_memory_with_config(config.clone()).unwrap();
        cassie.startup().unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        let server_cassie = cassie.clone();
        let server = tokio::spawn(async move {
            let _ = cassie::pgwire::server::run_with_listener(
                listener,
                Arc::new(server_cassie),
                config,
            )
            .await;
        });
        Self {
            addr,
            cassie,
            server,
        }
    }

    pub(super) async fn connect(&self) -> (tokio_postgres::Client, tokio::task::JoinHandle<()>) {
        let mut config = tokio_postgres::Config::new();
        config.host("127.0.0.1");
        config.port(self.addr.port());
        config.user("root");
        config.password("postgres");
        config.dbname("postgres");

        let (client, connection) = config.connect(NoTls).await.expect("connect tokio-postgres");
        let connection = tokio::spawn(async move {
            connection
                .await
                .expect("tokio-postgres connection task should stay healthy");
        });

        (client, connection)
    }

    pub(super) async fn shutdown(self, connection: tokio::task::JoinHandle<()>) {
        connection.abort();
        self.server.abort();
        let _ = connection.await;
        let _ = self.server.await;
    }

    async fn shutdown_without_client(self) {
        self.server.abort();
        let _ = self.server.await;
    }
}

pub(super) fn db_error(error: &tokio_postgres::Error) -> &DbError {
    error
        .as_db_error()
        .expect("tokio-postgres should return a database error")
}

fn first_simple_value(messages: Vec<SimpleQueryMessage>) -> String {
    messages
        .into_iter()
        .find_map(|message| match message {
            SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
            _ => None,
        })
        .expect("simple query should return a row")
}

pub(super) struct ProbeOutput {
    pub(super) success: bool,
    pub(super) timed_out: bool,
    pub(super) status_code: Option<i32>,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

pub(super) fn run_external_probe(
    mut command: Command,
    timeout: Duration,
) -> Result<ProbeOutput, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn external probe: {error}"))?;
    let started = Instant::now();

    loop {
        if child
            .try_wait()
            .map_err(|error| format!("poll external probe: {error}"))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("collect external probe output: {error}"))?;
            return Ok(ProbeOutput {
                success: output.status.success(),
                timed_out: false,
                status_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .map_err(|error| format!("collect timed-out external probe output: {error}"))?;
            return Ok(ProbeOutput {
                success: false,
                timed_out: true,
                status_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        thread::sleep(Duration::from_millis(50));
    }
}

include!("semantics.rs");

include!("metadata.rs");

include!("recovery.rs");
