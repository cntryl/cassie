//! PostgreSQL 18 string-function parameter and result metadata tracked by #803.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_sql_fixture::{sql_fixture_with_config, SqlFixture};
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::AsyncWriteExt;

#[path = "string/format_parameter_wires.rs"]
mod format_parameter_wires;
#[path = "string/parse_ident/semantics.rs"]
mod parse_ident_residuals;
#[path = "string/strpos_result_identity.rs"]
mod strpos_result_identity;
#[path = "string/text_selection.rs"]
mod text_selection;

fn configured_fixture(label: &str, maximum_rows: usize) -> (SqlFixture, CassieRuntimeConfig) {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = maximum_rows;
    let storage = sql_fixture_with_config(label, &[], config.clone());
    storage
        .cassie
        .startup()
        .expect("start set-returning function fixture");
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

include!("string/parameters.rs");

include!("string/errors.rs");

include!("string/nulls.rs");

include!("string/metadata.rs");

include!("string/semantics.rs");

include!("string/resources.rs");

include!("string/cancellation.rs");
