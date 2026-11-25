use cassie::app::Cassie;
use cassie::types::{Value, Vector};

use super::support_pgwire as support;

type WireFrame = (u8, Vec<u8>);

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn seed_result_codecs(cassie: &Cassie) {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE binary_codec_docs (small SMALLINT, regular INT, wide BIGINT, ratio FLOAT, flag BOOLEAN, blob BYTEA, item_uuid UUID, created_on DATE, created_at TIME, created_at_ts TIMESTAMP, title TEXT, code CHAR(4), alias VARCHAR(8), payload JSON, nullable TEXT)",
            Vec::new(),
        )
        .expect("create binary codec table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO binary_codec_docs (small, regular, wide, ratio, flag, blob, item_uuid, created_on, created_at, created_at_ts, title, code, alias, payload, nullable) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
            vec![
                Value::Int64(-12),
                Value::Int64(123_456_789),
                Value::Int64(-9_223_372_036_854_775_000),
                Value::Float64(3.5),
                Value::Bool(true),
                Value::String("\\x01020a".to_string()),
                Value::String("550e8400-e29b-41d4-a716-446655440000".to_string()),
                Value::String("2000-01-02".to_string()),
                Value::String("00:00:01.000002".to_string()),
                Value::String("2000-01-02T00:00:01.000003Z".to_string()),
                Value::String("text".to_string()),
                Value::String("ABCD".to_string()),
                Value::String("varchar".to_string()),
                Value::Json(serde_json::json!({"a": 1})),
                Value::Null,
            ],
        )
        .expect("insert binary codec row");
}

fn seed_parameter_codecs(cassie: &Cassie) {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE binary_parameter_docs (item_uuid UUID, created_on DATE, created_at TIME, created_at_ts TIMESTAMP, blob BYTEA)",
            Vec::new(),
        )
        .expect("create binary parameter table");
}

fn seed_unsupported_codecs(cassie: &Cassie) {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE binary_unsupported_docs (embedding VECTOR(2), values INT[])",
            Vec::new(),
        )
        .expect("create unsupported codec table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO binary_unsupported_docs (embedding, values) VALUES ($1, $2)",
            vec![
                Value::Vector(Vector::new(vec![1.0, 2.0])),
                Value::Json(serde_json::json!([1, 2])),
            ],
        )
        .expect("insert unsupported codec row");
}

async fn start_extended_query(
    cassie: Cassie,
    statement: Vec<u8>,
    bind: Vec<u8>,
    execute: Vec<u8>,
) -> (Vec<WireFrame>, support::PgwireServer) {
    let server = support::spawn_server(cassie).await;
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect pgwire");
    let (mut reader, mut writer) = tokio::io::split(socket);
    support::complete_startup(&mut reader, &mut writer).await;
    support::write_frames(
        &mut writer,
        vec![statement, bind, execute, support::sync_frame()],
    )
    .await;
    let frames = support::read_frames_until_ready(&mut reader).await;
    (frames, server)
}

fn read_binary_row(payload: &[u8]) -> Vec<Option<Vec<u8>>> {
    let mut cursor = 0usize;
    let field_count = read_i16(payload, &mut cursor);
    let mut values = Vec::new();
    for _ in 0..field_count {
        let length = read_i32(payload, &mut cursor);
        if length < 0 {
            values.push(None);
            continue;
        }
        let length = usize::try_from(length).expect("data row length");
        let end = cursor + length;
        values.push(Some(payload[cursor..end].to_vec()));
        cursor = end;
    }
    values
}

fn read_i16(payload: &[u8], cursor: &mut usize) -> i16 {
    let end = cursor.saturating_add(2);
    let bytes = payload[*cursor..end].try_into().expect("i16 payload");
    *cursor = end;
    i16::from_be_bytes(bytes)
}

fn read_i32(payload: &[u8], cursor: &mut usize) -> i32 {
    let end = cursor.saturating_add(4);
    let bytes = payload[*cursor..end].try_into().expect("i32 payload");
    *cursor = end;
    i32::from_be_bytes(bytes)
}

include!("metadata.rs");

include!("errors.rs");

include!("binary.rs");

include!("parameters.rs");

fn empty_int_array_body() -> Vec<u8> {
    [
        0_i32.to_be_bytes().as_slice(),
        0_i32.to_be_bytes().as_slice(),
        23_i32.to_be_bytes().as_slice(),
    ]
    .concat()
}

include!("nulls.rs");
