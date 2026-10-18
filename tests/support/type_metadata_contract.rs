//! Public pgwire fixtures for the finite, selected #752 type contract.

use crate::support_pgwire as wire;
use cassie::app::Cassie;
use cassie::types::Value;
use tokio::io::AsyncWriteExt;

#[path = "wire_bind.rs"]
mod bind;
#[path = "wire_queries.rs"]
mod queries;

pub use bind::{data_row_payloads, execute_cycle};
pub use queries::{assert_success, run_cycles, Cycle, Frames};

pub fn assert_unsupported_before_metadata(frames: &[(u8, Vec<u8>)]) {
    assert_eq!(wire::error_code(frames).as_deref(), Some("0A000"));
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    assert_eq!(
        frames
            .iter()
            .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
            .count(),
        0
    );
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
}

pub fn assert_single_column_descriptors(
    frames: &[(u8, Vec<u8>)],
    expected: (i32, i16, i32),
    result_format: i16,
    described: bool,
) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), if described { 2 } else { 1 });
    for (index, columns) in descriptions.into_iter().enumerate() {
        assert_eq!(columns.len(), 1);
        let column = &columns[0];
        assert_eq!(
            (column.type_oid, column.type_size, column.type_mod),
            expected
        );
        assert_eq!(
            column.format_code,
            if described && index == 0 {
                0
            } else {
                result_format
            }
        );
    }
}

pub fn one_field_row(field: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 1];
    bytes.extend_from_slice(
        &i32::try_from(field.len())
            .expect("fixture field")
            .to_be_bytes(),
    );
    bytes.extend_from_slice(field);
    bytes
}

pub fn vector_array_setup() -> Vec<(&'static str, Vec<Value>)> {
    vec![
        (
            "CREATE TABLE metadata_vector_empty (vectors VECTOR(7016)[])",
            Vec::new(),
        ),
        (
            "CREATE TABLE metadata_vector_rows (k INT, vectors VECTOR(7016)[])",
            Vec::new(),
        ),
        (
            "INSERT INTO metadata_vector_rows (k, vectors) VALUES ($1, $2)",
            vec![Value::Int64(1), Value::Null],
        ),
        (
            "INSERT INTO metadata_vector_rows (k, vectors) VALUES ($1, $2)",
            vec![Value::Int64(2), Value::Json(serde_json::json!([]))],
        ),
        (
            "CREATE TABLE metadata_boolean_rows (flags BOOLEAN[])",
            Vec::new(),
        ),
        (
            "INSERT INTO metadata_boolean_rows (flags) VALUES ($1)",
            vec![Value::Json(serde_json::json!([false, null, true]))],
        ),
    ]
}

pub fn run_cycles_across_restart(
    label: &str,
    setup: &[(&str, Vec<Value>)],
    stages: Vec<Vec<Cycle>>,
) -> Vec<Vec<Frames>> {
    wire::use_local_storage();
    let path = wire::data_dir(label);
    let mut results = Vec::new();
    for (index, cycles) in stages.into_iter().enumerate() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime for restart stage");
        let frames = runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir(&path).expect("Cassie at selected restart stage");
            cassie.startup().expect("startup at selected restart stage");
            if index == 0 {
                let session = cassie.create_session("tester", None);
                for (sql, params) in setup {
                    cassie
                        .execute_sql(&session, sql, params.clone())
                        .expect("seed restart fixture");
                }
            }
            exchange_cycles(cassie, cycles).await
        });
        // Drop all connection/background tasks before opening a second writer.
        drop(runtime);
        results.push(frames);
    }
    let _ = std::fs::remove_dir_all(path);
    results
}

async fn exchange_cycles(cassie: Cassie, cycles: Vec<Cycle>) -> Vec<Frames> {
    let server = wire::spawn_server(cassie).await;
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect pgwire");
    let (mut reader, mut writer) = tokio::io::split(socket);
    wire::complete_startup(&mut reader, &mut writer).await;
    let mut batches = Vec::new();
    for cycle in cycles {
        wire::write_frames(&mut writer, cycle).await;
        batches.push(
            wire::read_frames_until_ready_within(&mut reader, std::time::Duration::from_secs(5))
                .await,
        );
    }
    writer
        .shutdown()
        .await
        .expect("close restart-stage pgwire client");
    drop(reader);
    drop(writer);
    server.stop().await;
    tokio::task::yield_now().await;
    batches
}
