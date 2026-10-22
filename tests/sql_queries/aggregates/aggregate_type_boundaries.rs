//! PostgreSQL 18.6 residual aggregate and window boundaries for #806.

use cassie::types::Value;
use std::time::Duration;

use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture;

type Frames = Vec<(u8, Vec<u8>)>;

fn run_invalid_percentile_and_recovery() -> Vec<Frames> {
    let path = wire::data_dir("issue-806-percentile-invalid-fraction-sqlstate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    let batches = runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
        cassie.startup().expect("start Cassie before pgwire queries");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect to Cassie pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        let mut batches = Vec::with_capacity(2);
        for sql in [
            "SELECT percentile_cont(-0.1) WITHIN GROUP (ORDER BY value::FLOAT8) FROM (VALUES (1)) AS input(value)",
            "SELECT 42",
        ] {
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
            batches.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }

        drop(reader);
        drop(writer);
        server.stop().await;
        batches
    });

    let _ = std::fs::remove_dir_all(path);
    batches
}

include!("bit_bytea/aggregate_type_and_window_boundaries/nulls.rs");

include!("ordered_set/aggregate_type_and_window_boundaries/metadata.rs");

include!("../statistics/aggregate_type_and_window_boundaries/semantics.rs");

include!("ordered_set/aggregate_type_and_window_boundaries/nulls.rs");

include!("windows/aggregate_type_and_window_boundaries/nulls.rs");

include!("ordered_set/aggregate_type_and_window_boundaries/errors.rs");

include!("../sequences/aggregate_type_and_window_boundaries/semantics.rs");

include!("statistical/aggregate_type_and_window_boundaries/semantics.rs");
