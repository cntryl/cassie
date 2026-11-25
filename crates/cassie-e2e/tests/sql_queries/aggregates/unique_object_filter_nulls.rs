//! FILTER, ordering, strict NULL omission, and unique-key checks for JSON objects.

use cassie::types::Value;
use std::time::Duration;

use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture;

type Frames = Vec<(u8, Vec<u8>)>;

fn run_queries(label: &str, setup: &[&str], queries: &[&str]) -> Vec<Frames> {
    let path = wire::data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    let batches = runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
        cassie.startup().expect("start Cassie before setup");
        let session = cassie.create_session("tester", None);
        for sql in setup {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed unique JSON aggregate fixture");
        }

        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect to Cassie pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        let mut batches = Vec::with_capacity(queries.len());
        for sql in queries {
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

include!("windows/unique_object_aggregate_filter_null/semantics.rs");

include!("json/unique_object_aggregate_filter_null/errors.rs");
