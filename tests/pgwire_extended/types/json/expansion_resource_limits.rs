//! Bounded, cancellable JSON row expansion and atomic failure publication.

#[path = "table_functions/recordset_midstream_cancellation.rs"]
mod recordset_midstream_cancellation;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::AsyncWriteExt;

use crate::support_pgwire as wire;
use crate::support_sql_fixture::{sql_fixture_with_config, SqlFixture};
use crate::support_type_metadata_contract as fixture;

fn configured_fixture(label: &str, maximum_rows: usize) -> (SqlFixture, CassieRuntimeConfig) {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = maximum_rows;
    let storage = sql_fixture_with_config(label, &[], config.clone());
    storage
        .cassie
        .startup()
        .expect("start JSON expansion fixture");
    (storage, config)
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

async fn start_server(
    instance: Cassie,
    config: CassieRuntimeConfig,
) -> (SocketAddr, tokio::task::JoinHandle<Result<(), CassieError>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind pgwire listener");
    let address = listener.local_addr().expect("listener address");
    drop(listener);
    let server = tokio::spawn(cassie::pgwire::server::run(
        address.to_string(),
        Arc::new(instance),
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    (address, server)
}
