#[path = "support/pgwire.rs"]
mod pgwire;

use std::time::Duration;

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use pgwire::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn password_message_with_len(length: i32) -> Vec<u8> {
    let mut frame = vec![b'p'];
    frame.extend_from_slice(&length.to_be_bytes());
    frame
}

fn startup_frame_with_len(length: i32) -> Vec<u8> {
    length.to_be_bytes().to_vec()
}

fn error_code(payload: &[u8]) -> Option<String> {
    parse_error_fields(payload)
        .into_iter()
        .find_map(|(field, value)| (field == 'C').then_some(value))
}

fn score_schema() -> Schema {
    Schema {
        fields: vec![FieldSchema {
            name: "score".to_string(),
            data_type: DataType::Int,
            nullable: true,
        }],
    }
}

fn seed_scores(cassie: &Cassie, collection: &str) {
    let collection = canonical_relation_name("postgres", "public", collection);
    let schema = score_schema();
    cassie
        .midge
        .create_collection(&collection, schema.clone())
        .expect("create collection");
    cassie.register_collection(&collection, schema);
    for (id, score) in [("doc-1", 1), ("doc-2", 2), ("doc-3", 3)] {
        cassie
            .midge
            .put_document(
                &collection,
                Some(id.to_string()),
                serde_json::json!({ "score": score }),
            )
            .expect("put document");
    }
}

fn seed_binary_bind_docs(cassie: &Cassie, collection: &str) {
    let collection = canonical_relation_name("postgres", "public", collection);
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "flag".to_string(),
                data_type: DataType::Boolean,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "ratio".to_string(),
                data_type: DataType::Float,
                nullable: true,
            },
        ],
    };
    cassie
        .midge
        .create_collection(&collection, schema.clone())
        .expect("create collection");
    cassie.register_collection(&collection, schema);
    cassie
        .midge
        .put_document(
            &collection,
            Some("doc-1".to_string()),
            serde_json::json!({ "flag": true, "score": 7, "ratio": 3.5 }),
        )
        .expect("put document");
}

#[test]
fn should_close_connection_after_oversized_startup_frame() {
    // Arrange
    with_fallback();
    let path = data_dir("oversized_startup");
    let runtime = runtime();

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");

        // Act
        socket
            .write_all(&startup_frame_with_len(16 * 1024 * 1024 + 5))
            .await
            .expect("write oversized startup");
        let mut tag = [0u8; 1];
        socket.read_exact(&mut tag).await.expect("read error tag");
        let mut length = [0u8; 4];
        socket
            .read_exact(&mut length)
            .await
            .expect("read error length");
        let payload_len = usize::try_from(i32::from_be_bytes(length) - 4).expect("payload len");
        let mut payload = vec![0u8; payload_len];
        socket
            .read_exact(&mut payload)
            .await
            .expect("read error payload");
        let closed = tokio::time::timeout(Duration::from_secs(1), socket.read_u8()).await;

        // Assert
        assert_eq!(tag[0], b'E');
        assert_eq!(error_code(&payload).as_deref(), Some("08P01"));
        assert!(
            closed.is_ok(),
            "fatal startup error should close the socket"
        );

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_close_connection_after_oversized_password_frame() {
    // Arrange
    with_fallback();
    let path = data_dir("oversized_password");
    let runtime = runtime();

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "secret".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
        cassie.startup().expect("startup");
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
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");

        // Act
        socket
            .write_all(&startup_frame("postgres", "postgres"))
            .await
            .expect("write startup");
        let auth = {
            let mut tag = [0u8; 1];
            socket.read_exact(&mut tag).await.expect("read auth tag");
            tag[0]
        };
        let mut auth_len = [0u8; 4];
        socket
            .read_exact(&mut auth_len)
            .await
            .expect("read auth length");
        let mut auth_payload =
            vec![0u8; usize::try_from(i32::from_be_bytes(auth_len) - 4).expect("auth len")];
        socket
            .read_exact(&mut auth_payload)
            .await
            .expect("read auth payload");
        socket
            .write_all(&password_message_with_len(16 * 1024 * 1024 + 5))
            .await
            .expect("write oversized password");
        let mut error_tag = [0u8; 1];
        socket
            .read_exact(&mut error_tag)
            .await
            .expect("read error tag");
        let closed = tokio::time::timeout(Duration::from_secs(1), socket.read_u8()).await;

        // Assert
        assert_eq!(auth, b'R');
        assert_eq!(error_tag[0], b'E');
        assert!(
            closed.is_ok(),
            "fatal password error should close the socket"
        );

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_duplicate_named_parse_until_sync() {
    // Arrange
    with_fallback();
    let path = data_dir("duplicate_parse");
    let runtime = runtime();

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = socket.split();
        complete_startup(&mut reader, &mut writer).await;

        // Act
        write_frames(
            &mut writer,
            vec![
                parse_frame("stmt", "SELECT version()"),
                parse_frame("stmt", "SELECT current_database()"),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(frames[0].0, b'1');
        assert_eq!(frames[1].0, b'E');
        assert_eq!(error_code(&frames[1].1).as_deref(), Some("08P01"));
        assert_eq!(frames[2].0, b'Z');

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_invalidate_portals_when_unnamed_statement_is_replaced() {
    // Arrange
    with_fallback();
    let path = data_dir("unnamed_replace");
    let runtime = runtime();

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = socket.split();
        complete_startup(&mut reader, &mut writer).await;

        // Act
        write_frames(
            &mut writer,
            vec![
                parse_frame("", "SELECT version()"),
                bind_frame("portal", "", &[]),
                parse_frame("", "SELECT current_database()"),
                execute_frame("portal"),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(frames[0].0, b'1');
        assert_eq!(frames[1].0, b'2');
        assert_eq!(frames[2].0, b'1');
        assert_eq!(frames[3].0, b'E');
        assert_eq!(error_code(&frames[3].1).as_deref(), Some("26000"));
        assert_eq!(frames[4].0, b'Z');

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_unsupported_bind_format_codes() {
    // Arrange
    with_fallback();
    let path = data_dir("bad_formats");
    let runtime = runtime();

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        seed_scores(&cassie, "bad_format_scores");
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = socket.split();
        complete_startup(&mut reader, &mut writer).await;

        // Act
        write_frames(
            &mut writer,
            vec![
                parse_frame(
                    "stmt",
                    "SELECT score FROM bad_format_scores WHERE score = $1",
                ),
                bind_frame_with_formats("portal", "stmt", &[2], &[Some(b"value")], &[]),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(frames[0].0, b'1');
        assert_eq!(frames[1].0, b'E');
        assert_eq!(error_code(&frames[1].1).as_deref(), Some("08P01"));
        assert_eq!(frames[2].0, b'Z');

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_portal_result_format_to_row_description() {
    // Arrange
    with_fallback();
    let path = data_dir("rowdesc_binary");
    let runtime = runtime();

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        seed_scores(&cassie, "rowdesc_scores");
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = socket.split();
        complete_startup(&mut reader, &mut writer).await;

        // Act
        write_frames(
            &mut writer,
            vec![
                parse_frame("stmt", "SELECT score FROM rowdesc_scores ORDER BY score"),
                bind_frame_with_formats("portal", "stmt", &[], &[], &[1]),
                describe_portal_frame("portal"),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(frames[0].0, b'1');
        assert_eq!(frames[1].0, b'2');
        assert_eq!(frames[2].0, b'T');
        let row_description = parse_row_description(&frames[2].1);
        assert_eq!(row_description[0].format_code, 1);

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_decode_binary_bind_parameters_by_oid() {
    // Arrange
    with_fallback();
    let path = data_dir("binary_binds");
    let runtime = runtime();

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        seed_binary_bind_docs(&cassie, "binary_bind_docs");
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = socket.split();
        complete_startup(&mut reader, &mut writer).await;

        // Act
        write_frames(
            &mut writer,
            vec![
                parse_frame_with_types(
                    "stmt",
                    "INSERT INTO binary_bind_docs (flag, score, ratio) VALUES ($1, $2, $3) RETURNING flag, score, ratio",
                    &[16, 23, 701],
                ),
                bind_frame_with_formats(
                    "portal",
                    "stmt",
                    &[1],
                    &[
                        Some(&[1]),
                        Some(&7_i32.to_be_bytes()),
                        Some(&3.5_f64.to_be_bytes()),
                    ],
                    &[],
                ),
                execute_frame("portal"),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        let row = frames.iter().find(|frame| frame.0 == b'D').expect("data row");
        assert_eq!(
            parse_data_row(&row.1),
            vec![
                Some("true".to_string()),
                Some("7".to_string()),
                Some("3.5".to_string()),
            ]
        );

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_resume_suspended_portal_after_execute_limit() {
    // Arrange
    with_fallback();
    let path = data_dir("portal_suspend");
    let runtime = runtime();

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        seed_scores(&cassie, "portal_suspend_scores");
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = socket.split();
        complete_startup(&mut reader, &mut writer).await;

        // Act
        write_frames(
            &mut writer,
            vec![
                parse_frame(
                    "stmt",
                    "SELECT score FROM portal_suspend_scores ORDER BY score",
                ),
                bind_frame("portal", "stmt", &[]),
                execute_limited_frame("portal", 2),
                execute_frame("portal"),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        let tags = frames.iter().map(|frame| frame.0).collect::<Vec<_>>();
        assert_eq!(
            tags,
            vec![b'1', b'2', b'T', b'D', b'D', b's', b'D', b'C', b'Z']
        );
        assert_eq!(parse_data_row(&frames[3].1), vec![Some("1".to_string())]);
        assert_eq!(parse_data_row(&frames[4].1), vec![Some("2".to_string())]);
        assert_eq!(parse_data_row(&frames[6].1), vec![Some("3".to_string())]);

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}
