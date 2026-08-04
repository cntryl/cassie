#![allow(unused_imports, dead_code)]
#[path = "support/pgwire.rs"]
mod pgwire_support;

use std::net::SocketAddr;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use pgwire_support::{
    bind_frame, data_dir, describe_statement_frame, execute_frame, parse_data_row, parse_frame,
    parse_parameter_description, read_until_ready, read_wire_frame, startup_frame, sync_frame,
    use_local_storage,
};

type WireFrame = (u8, Vec<u8>);
type PgwireReader<'a> = tokio::io::BufReader<tokio::net::tcp::ReadHalf<'a>>;
type PgwireWriter<'a> = tokio::net::tcp::WriteHalf<'a>;
type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

fn password_frame(password: &str) -> Vec<u8> {
    pgwire_support::password_message(password)
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

fn parse_row_description(payload: &[u8]) -> Vec<(String, i32, i16, i32, i16)> {
    pgwire_support::parse_row_description(payload)
        .into_iter()
        .map(|field| {
            (
                field.name,
                field.table_oid,
                field.type_size,
                field.type_oid,
                field.format_code,
            )
        })
        .collect()
}

fn seed_extended_query_collection(cassie: &Cassie) {
    let collection = canonical_relation_name("postgres", "public", "extended_query_docs");
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
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
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
    let auth = read_wire_frame(reader).await;
    assert_eq!(auth.0, b'R', "startup should return an auth response");
    assert_eq!(
        i32::from_be_bytes(auth.1[0..4].try_into().expect("auth payload")),
        3,
        "startup should request a cleartext password"
    );
    tokio::io::AsyncWriteExt::write_all(writer, &password_frame("postgres"))
        .await
        .expect("write password");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush password");
    let auth_ok = read_wire_frame(reader).await;
    assert_eq!(auth_ok.0, b'R', "password should return an auth response");
    assert_eq!(
        i32::from_be_bytes(auth_ok.1[0..4].try_into().expect("auth payload")),
        0,
        "password auth should succeed"
    );
    let startup_ready = read_until_ready(reader).await;
    assert_eq!(startup_ready, vec![b'I']);
}

async fn write_extended_lifecycle_batch(writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &parse_frame(
            "stmt_extended_lifecycle",
            "SELECT title FROM extended_query_docs WHERE title = $1 ORDER BY title",
        ),
    )
    .await
    .expect("write parse");
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &describe_statement_frame("stmt_extended_lifecycle"),
    )
    .await
    .expect("write describe");
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &bind_frame(
            "portal_extended_lifecycle",
            "stmt_extended_lifecycle",
            &["alpha"],
        ),
    )
    .await
    .expect("write bind");
    tokio::io::AsyncWriteExt::write_all(writer, &execute_frame("portal_extended_lifecycle"))
        .await
        .expect("write execute");
    tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
        .await
        .expect("write sync");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush frames");
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

fn assert_extended_lifecycle_frames(frames: &[WireFrame]) {
    assert_eq!(
        frames.len(),
        7,
        "extended query should return seven backend frames"
    );
    assert_eq!(frames[0].0, b'1', "parse should complete first");
    assert_eq!(
        frames[1].0, b't',
        "describe should return parameter metadata first"
    );
    assert_eq!(frames[2].0, b'T', "describe should return row metadata");
    assert_eq!(frames[3].0, b'2', "bind should complete after describe");
    assert_eq!(frames[4].0, b'D', "execute should return a data row");
    assert_eq!(
        frames[5].0, b'C',
        "execute should end with command complete"
    );
    assert_eq!(frames[6].0, b'Z', "sync should finish with ready-for-query");

    let parameters = parse_parameter_description(&frames[1].1);
    assert_eq!(parameters, vec![25]);

    let fields = parse_row_description(&frames[2].1);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].0, "title");
    assert_eq!(fields[0].3, 25, "text columns should use the text OID");

    let values = parse_data_row(&frames[4].1);
    assert_eq!(values, vec![Some("alpha".to_string())]);

    let mut command_cursor = 0usize;
    let command = read_cstring(&frames[5].1, &mut command_cursor);
    assert!(
        command.starts_with("SELECT"),
        "command completion should identify the select command"
    );
    assert_eq!(frames[6].1, vec![b'I']);
}

#[test]
fn should_execute_binary_extended_query_lifecycle_return_backend_frames() {
    // Arrange
    use_local_storage();
    let path = data_dir("lifecycle");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        seed_extended_query_collection(&cassie);

        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        write_extended_lifecycle_batch(&mut write_half).await;
        let frames = read_ready_frames(&mut reader).await;

        // Assert
        assert_extended_lifecycle_frames(&frames);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
