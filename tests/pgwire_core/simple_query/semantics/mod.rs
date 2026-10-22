use super::support_pgwire as pgwire_support;

use std::net::SocketAddr;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use pgwire_support::{
    copy_data_frame, copy_done_frame, data_dir, parse_data_row, parse_error_fields,
    password_message, read_until_ready, read_wire_frame, simple_query_frame, startup_frame,
};

type WireFrame = (u8, Vec<u8>);
type PgwireReader<'a> = tokio::io::BufReader<tokio::net::tcp::ReadHalf<'a>>;
type PgwireWriter<'a> = tokio::net::tcp::WriteHalf<'a>;
type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn read_cstring(payload: &[u8], cursor: &mut usize) -> String {
    let tail = payload
        .get(*cursor..)
        .expect("cursor should be inside payload");
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .expect("cstring should be null terminated");
    let value = std::str::from_utf8(&tail[..end]).expect("cstring should be utf-8");
    *cursor += end + 1;
    value.to_string()
}

fn read_i16(payload: &[u8], cursor: &mut usize) -> i16 {
    let start = *cursor;
    let end = start + 2;
    let bytes: [u8; 2] = payload[start..end].try_into().expect("i16 payload");
    *cursor = end;
    i16::from_be_bytes(bytes)
}

fn read_i32(payload: &[u8], cursor: &mut usize) -> i32 {
    let start = *cursor;
    let end = start + 4;
    let bytes: [u8; 4] = payload[start..end].try_into().expect("i32 payload");
    *cursor = end;
    i32::from_be_bytes(bytes)
}

fn parse_row_description(payload: &[u8]) -> Vec<(String, i32, i16, i32, i16)> {
    let mut cursor = 0usize;
    let field_count = read_i16(payload, &mut cursor);
    let mut fields = Vec::new();

    for _ in 0..field_count {
        let name = read_cstring(payload, &mut cursor);
        let table_oid = read_i32(payload, &mut cursor);
        let _attr_num = read_i16(payload, &mut cursor);
        let type_oid = read_i32(payload, &mut cursor);
        let type_size = read_i16(payload, &mut cursor);
        let _type_mod = read_i32(payload, &mut cursor);
        let format_code = read_i16(payload, &mut cursor);
        fields.push((name, table_oid, type_size, type_oid, format_code));
    }

    fields
}

fn error_field(fields: &[(char, String)], tag: char) -> Option<&str> {
    fields
        .iter()
        .find(|(field, _)| *field == tag)
        .map(|(_, value)| value.as_str())
}

fn seed_copy_collection(cassie: &Cassie) {
    let collection = canonical_relation_name("postgres", "public", "simple_copy_docs");
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: false,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
        ],
    };
    cassie
        .midge
        .create_collection(&collection, schema.clone())
        .unwrap();
    cassie.register_collection(&collection, schema);
}

fn seed_simple_query_collection(cassie: &Cassie) {
    let collection = canonical_relation_name("postgres", "public", "simple_query_docs");
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection(&collection, schema.clone())
        .unwrap();
    cassie.register_collection(&collection, schema);
    cassie
        .midge
        .put_document(
            &collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .unwrap();
}

async fn spawn_pgwire_server(cassie: &Cassie) -> (SocketAddr, PgwireServer) {
    spawn_pgwire_server_with_config(
        cassie,
        CassieRuntimeConfig::from_env().expect("runtime config"),
    )
    .await
}

async fn spawn_pgwire_server_with_config(
    cassie: &Cassie,
    mut config: CassieRuntimeConfig,
) -> (SocketAddr, PgwireServer) {
    config.password = "postgres".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener address");
    drop(listener);

    let server = tokio::spawn(cassie::pgwire::server::run(
        addr.to_string(),
        std::sync::Arc::new(cassie.clone()),
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, server)
}

async fn start_pgwire_session(reader: &mut PgwireReader<'_>, writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(writer, &startup_frame("root", "postgres"))
        .await
        .expect("write startup");
    let (auth_tag, auth_payload) = read_wire_frame(reader).await;
    assert_eq!(auth_tag, b'R', "startup should return an auth response");
    let auth_status = i32::from_be_bytes(auth_payload[0..4].try_into().expect("auth status"));
    match auth_status {
        0 => {}
        3 => {
            tokio::io::AsyncWriteExt::write_all(writer, &password_message("postgres"))
                .await
                .expect("write password");
            tokio::io::AsyncWriteExt::flush(writer)
                .await
                .expect("flush password");
            let (auth_ok_tag, auth_ok_payload) = read_wire_frame(reader).await;
            assert_eq!(auth_ok_tag, b'R', "auth success should use auth response");
            assert_eq!(
                i32::from_be_bytes(auth_ok_payload[0..4].try_into().expect("auth ok status")),
                0,
                "cleartext auth should accept the configured password"
            );
        }
        other => panic!("unexpected auth status {other}"),
    }
    let startup_ready = read_until_ready(reader).await;
    assert_eq!(startup_ready, vec![b'I']);
}

async fn read_ready_frames(reader: &mut PgwireReader<'_>) -> Vec<WireFrame> {
    let mut frames = Vec::new();
    loop {
        let frame = read_wire_frame(reader).await;
        let tag = frame.0;
        frames.push(frame);
        if tag == b'Z' {
            return frames;
        }
    }
}

async fn write_simple_query_and_read_frames(
    reader: &mut PgwireReader<'_>,
    writer: &mut PgwireWriter<'_>,
    sql: &str,
) -> Vec<WireFrame> {
    tokio::io::AsyncWriteExt::write_all(writer, &simple_query_frame(sql))
        .await
        .expect("write query");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush query");
    read_ready_frames(reader).await
}

include!("semantics.rs");

async fn request_copy_from_stdin(reader: &mut PgwireReader<'_>, writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &simple_query_frame(
            "COPY simple_copy_docs (_id, title, score) FROM STDIN WITH (FORMAT csv, HEADER true)",
        ),
    )
    .await
    .expect("write copy query");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush copy query");

    let copy_in = read_wire_frame(reader).await;
    assert_eq!(copy_in.0, b'G', "copy should return CopyInResponse");
    assert_eq!(copy_in.1[0], 0, "COPY should use text format");
    assert_eq!(
        i16::from_be_bytes(copy_in.1[1..3].try_into().expect("copy column count")),
        3
    );
}

async fn send_copy_rows(
    reader: &mut PgwireReader<'_>,
    writer: &mut PgwireWriter<'_>,
) -> (WireFrame, WireFrame) {
    let copy_payload = b"_id,title,score\ncopy-1,alpha,7\ncopy-2,beta,9\n";
    tokio::io::AsyncWriteExt::write_all(writer, &copy_data_frame(copy_payload))
        .await
        .expect("write copy data");
    tokio::io::AsyncWriteExt::write_all(writer, &copy_done_frame())
        .await
        .expect("write copy done");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush copy data");

    let complete = read_wire_frame(reader).await;
    let ready = read_wire_frame(reader).await;
    (complete, ready)
}

fn assert_copy_complete_frames(complete: &WireFrame, ready: &WireFrame) {
    assert_eq!(
        complete.0,
        b'C',
        "copy should complete command: {:?}",
        parse_error_fields(&complete.1)
    );
    let mut command_cursor = 0usize;
    assert_eq!(read_cstring(&complete.1, &mut command_cursor), "COPY 2");
    assert_eq!(ready.0, b'Z');
    assert_eq!(ready.1, vec![b'I']);
}

fn assert_copy_select_frames(frames: &[WireFrame]) {
    assert_eq!(frames[0].0, b'T');
    assert_eq!(frames[1].0, b'D');
    assert_eq!(frames[2].0, b'D');
    assert_eq!(
        parse_data_row(&frames[1].1),
        vec![Some("alpha".to_string()), Some("7".to_string())]
    );
    assert_eq!(
        parse_data_row(&frames[2].1),
        vec![Some("beta".to_string()), Some("9".to_string())]
    );
    assert_eq!(frames[3].0, b'C');
    assert_eq!(frames[4].0, b'Z');
}

fn assert_simple_query_backend_frames(frames: &[WireFrame]) {
    assert_eq!(
        frames.len(),
        4,
        "simple query should return four backend frames"
    );
    assert_eq!(frames[0].0, b'T', "first frame should be row description");
    assert_eq!(frames[1].0, b'D', "second frame should be a data row");
    assert_eq!(frames[2].0, b'C', "third frame should be command complete");
    assert_eq!(frames[3].0, b'Z', "final frame should be ready for query");

    let fields = parse_row_description(&frames[0].1);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].0, "title");
    assert_eq!(fields[0].3, 25, "text columns should use the text OID");

    let values = parse_data_row(&frames[1].1);
    assert_eq!(values, vec![Some("alpha".to_string())]);

    let mut command_cursor = 0usize;
    let command = read_cstring(&frames[2].1, &mut command_cursor);
    assert!(
        command.starts_with("SELECT"),
        "command completion should identify the select command"
    );
    assert_eq!(frames[3].1, vec![b'I']);
}

include!("nulls.rs");

fn seed_copy_export_rows(cassie: &Cassie) {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE pgwire_copy_export_rows (id INT, title TEXT, note TEXT)",
            vec![],
        )
        .expect("create COPY export table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO pgwire_copy_export_rows VALUES \
                   (1, 'alpha', NULL), (2, 'a,comma', 'present')",
            vec![],
        )
        .expect("seed COPY export rows");
}

fn copy_out_bytes(frames: &[WireFrame]) -> Vec<u8> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'd')
        .flat_map(|(_, payload)| payload.iter().copied())
        .collect()
}

fn assert_copy_out_response(frames: &[WireFrame], format: i16, columns: usize) {
    let response = frames
        .iter()
        .find(|(tag, _)| *tag == b'H')
        .unwrap_or_else(|| panic!("COPY TO STDOUT should return CopyOutResponse: {frames:?}"));
    assert_eq!(response.1[0], u8::try_from(format).expect("format code"));
    let column_count = usize::try_from(i16::from_be_bytes(
        response.1[1..3].try_into().expect("COPY column count"),
    ))
    .expect("nonnegative COPY column count");
    assert_eq!(column_count, columns);
    let (format_pairs, remainder) = response.1[3..].as_chunks::<2>();
    assert!(
        remainder.is_empty(),
        "COPY format list must contain full pairs"
    );
    let formats = format_pairs
        .iter()
        .map(|chunk| i16::from_be_bytes(*chunk))
        .collect::<Vec<_>>();
    assert_eq!(formats, vec![format; columns]);
    assert!(frames.iter().any(|(tag, _)| *tag == b'c'));
    let command = frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .expect("COPY TO STDOUT should return CommandComplete");
    let mut cursor = 0;
    assert_eq!(read_cstring(&command.1, &mut cursor), "COPY 2");
    assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
}

include!("resources.rs");

include!("binary.rs");

include!("errors.rs");

include!("cancellation.rs");
