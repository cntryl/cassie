//! Explicit Psycopg qualification fixture. Requires the documented pinned venv.

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use std::sync::Arc;

fn main() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("qualification runtime")
        .block_on(qualify());
}

async fn qualify() {
    let python = std::env::var_os("CASSIE_GRAPH_PYTHON")
        .expect("set CASSIE_GRAPH_PYTHON to the pinned qualification venv's Python");
    let config = CassieRuntimeConfig {
        password: uuid::Uuid::new_v4().to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_in_memory_with_config(config.clone()).expect("memory fixture");
    cassie.startup().expect("fixture startup");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned listener");
    let port = listener.local_addr().expect("listener address").port();
    let server = tokio::spawn(cassie::pgwire::server::run_with_listener(
        listener,
        Arc::new(cassie),
        config.clone(),
    ));
    let mut command = tokio::process::Command::new(python);
    command
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/qualification/graph_psycopg.py"
        ))
        .env("CASSIE_GRAPH_PORT", port.to_string())
        .env("CASSIE_GRAPH_PASSWORD", &config.password)
        .kill_on_drop(true);
    let status = tokio::time::timeout(std::time::Duration::from_secs(60), command.status()).await;
    server.abort();
    let _ = server.await;
    assert!(
        status
            .expect("qualification timeout")
            .expect("launch qualification Python")
            .success(),
        "Psycopg qualification failed"
    );
}
