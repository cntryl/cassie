// Consolidated integration suite: pgwire_extended.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/pgwire_extended_control.rs.
mod pgwire_extended_control {
    #![allow(unused_imports, dead_code)]

    use super::support_pgwire as pgwire_support;

    use std::net::SocketAddr;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::{DataType, FieldSchema, Schema};
    use pgwire_support::{
        bind_frame, cancel_request_frame, data_dir, describe_statement_frame, execute_frame,
        parse_data_row, parse_error_fields, parse_frame, parse_parameter_description,
        parse_row_description, read_until_ready, read_wire_frame, startup_frame, sync_frame,
        use_local_storage,
    };

    type WireFrame = (u8, Vec<u8>);
    type PgwireReader<'a> = tokio::io::BufReader<tokio::net::tcp::ReadHalf<'a>>;
    type PgwireWriter<'a> = tokio::net::tcp::WriteHalf<'a>;
    type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

    fn password_frame(password: &str) -> Vec<u8> {
        pgwire_support::password_message(password)
    }

    fn frontend_frame(tag: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.push(tag);
        frame.extend_from_slice(
            &i32::try_from(payload.len() + 4)
                .expect("frontend payload size must fit into i32")
                .to_be_bytes(),
        );
        frame.extend_from_slice(payload);
        frame
    }

    fn close_frame(target: u8, name: &str) -> Vec<u8> {
        match target {
            b'S' => pgwire_support::close_statement_frame(name),
            b'P' => pgwire_support::close_portal_frame(name),
            _ => panic!("unsupported close target: {target}"),
        }
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

    fn seed_recovery_collection(cassie: &Cassie) {
        let collection =
            canonical_relation_name("postgres", "public", "extended_query_recovery_docs");
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
        let mut cursor = 0;
        assert_eq!(read_i32(&auth.1, &mut cursor), 3);
        tokio::io::AsyncWriteExt::write_all(writer, &password_frame("postgres"))
            .await
            .expect("write password");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush password");
        let auth_ok = read_wire_frame(reader).await;
        assert_eq!(auth_ok.0, b'R', "password should return an auth response");
        let mut cursor = 0;
        assert_eq!(read_i32(&auth_ok.1, &mut cursor), 0);
        let startup_ready = read_until_ready(reader).await;
        assert_eq!(startup_ready, vec![b'I']);
    }

    async fn write_parse_error_recovery_batch(writer: &mut PgwireWriter<'_>) {
        tokio::io::AsyncWriteExt::write_all(
            writer,
            &parse_frame("stmt_recovery_error", "SELECT * FROM"),
        )
        .await
        .expect("write invalid parse");
        tokio::io::AsyncWriteExt::write_all(
            writer,
            &parse_frame(
                "stmt_recovery_valid",
                "SELECT title FROM extended_query_recovery_docs WHERE title = $1 ORDER BY title",
            ),
        )
        .await
        .expect("write ignored parse");
        tokio::io::AsyncWriteExt::write_all(
            writer,
            &bind_frame("portal_recovery", "stmt_recovery_valid", &["alpha"]),
        )
        .await
        .expect("write ignored bind");
        tokio::io::AsyncWriteExt::write_all(writer, &execute_frame("portal_recovery"))
            .await
            .expect("write ignored execute");
        tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
            .await
            .expect("write sync");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush recovery batch");
    }

    fn assert_parse_error_recovery(error: &WireFrame, ready: &WireFrame) {
        assert_eq!(error.0, b'E', "parse failure should return an error frame");
        assert_eq!(
            ready.0, b'Z',
            "sync after a parse failure should restore ready-for-query"
        );
        assert_eq!(
            parse_error_fields(&error.1)
                .iter()
                .find(|(field, _)| *field == 'C')
                .map(|(_, value)| value.as_str()),
            Some("42601"),
            "parse failure should be reported as a syntax error"
        );
        assert_eq!(ready.1, vec![b'I']);
    }

    async fn write_recovered_query_batch(writer: &mut PgwireWriter<'_>) {
        tokio::io::AsyncWriteExt::write_all(
            writer,
            &parse_frame(
                "stmt_recovery_valid",
                "SELECT title FROM extended_query_recovery_docs WHERE title = $1 ORDER BY title",
            ),
        )
        .await
        .expect("write recovery parse");
        tokio::io::AsyncWriteExt::write_all(
            writer,
            &bind_frame("portal_recovery", "stmt_recovery_valid", &["alpha"]),
        )
        .await
        .expect("write recovery bind");
        tokio::io::AsyncWriteExt::write_all(writer, &execute_frame("portal_recovery"))
            .await
            .expect("write recovery execute");
        tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
            .await
            .expect("write recovery sync");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush recovery follow-up");
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

    fn assert_recovered_query_frames(frames: &[WireFrame]) {
        let tags = frames
            .iter()
            .map(|frame| char::from(frame.0))
            .collect::<String>();
        assert_eq!(
            frames.len(),
            6,
            "recovered query should execute normally, tags={tags}"
        );
        assert_eq!(frames[0].0, b'1');
        assert_eq!(frames[1].0, b'2');
        assert_eq!(frames[2].0, b'T');
        assert_eq!(frames[3].0, b'D');
        assert_eq!(frames[4].0, b'C');
        assert_eq!(frames[5].0, b'Z');
        assert_eq!(frames[5].1, vec![b'I']);

        let values = parse_data_row(&frames[3].1);
        assert_eq!(values, vec![Some("alpha".to_string())]);
    }

    #[test]
    fn should_close_connection_on_cancel_request_without_response() {
        // Arrange
        use_local_storage();
        let path = data_dir("cancel");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let config = CassieRuntimeConfig::from_env().expect("runtime config");
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
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                &cancel_request_frame(11_223_344, 55_667_788),
            )
            .await
            .expect("write cancel request");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush cancel request");

            let mut buffer = [0u8; 1];
            let read = tokio::time::timeout(
                Duration::from_secs(1),
                tokio::io::AsyncReadExt::read(&mut reader, &mut buffer),
            )
            .await
            .expect("cancel request should close promptly")
            .expect("read cancel response");

            // Assert
            assert_eq!(read, 0, "cancel request should not produce a response");

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_copy_data_message_with_unsupported_error() {
        // Arrange
        use_local_storage();
        let path = data_dir("copy_data");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
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
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            start_pgwire_session(&mut reader, &mut write_half).await;

            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                &frontend_frame(b'd', b"copy payload"),
            )
            .await
            .expect("write copy data");
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &sync_frame())
                .await
                .expect("write sync");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush copy data batch");

            let error = read_wire_frame(&mut reader).await;
            let ready = read_wire_frame(&mut reader).await;

            // Assert
            assert_eq!(
                error.0, b'E',
                "copy data should be rejected with an error frame"
            );
            assert_eq!(ready.0, b'Z', "sync after copy rejection should recover");
            assert_eq!(ready.1, vec![b'I']);
            let error_fields = parse_error_fields(&error.1);
            assert_eq!(
                error_fields
                    .iter()
                    .find(|(field, _)| *field == 'C')
                    .map(|(_, value)| value.as_str()),
                Some("0A000"),
                "copy data should return an unsupported-feature SQLSTATE"
            );

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_ignore_extended_query_messages_until_sync_after_parse_error() {
        // Arrange
        use_local_storage();
        let path = data_dir("recovery");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            seed_recovery_collection(&cassie);

            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            start_pgwire_session(&mut reader, &mut write_half).await;
            write_parse_error_recovery_batch(&mut write_half).await;
            let error = read_wire_frame(&mut reader).await;
            let ready = read_wire_frame(&mut reader).await;

            // Assert
            assert_parse_error_recovery(&error, &ready);
            write_recovered_query_batch(&mut write_half).await;
            let frames = read_ready_frames(&mut reader).await;
            assert_recovered_query_frames(&frames);

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_return_unsupported_error_for_copy_statement() {
        // Arrange
        use_local_storage();
        let path = data_dir("copy_unsupported");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
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
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            start_pgwire_session(&mut reader, &mut write_half).await;

            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                &parse_frame("stmt_copy", "COPY extended_query_close_docs TO STDOUT"),
            )
            .await
            .expect("write copy parse");
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &sync_frame())
                .await
                .expect("write sync");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush copy batch");

            let error = read_wire_frame(&mut reader).await;
            let ready = read_wire_frame(&mut reader).await;

            // Assert
            assert_eq!(error.0, b'E', "copy should be rejected with an error frame");
            assert_eq!(ready.0, b'Z', "sync after copy rejection should recover");
            assert_eq!(ready.1, vec![b'I']);
            let error_fields = parse_error_fields(&error.1);
            assert_eq!(
                error_fields
                    .iter()
                    .find(|(field, _)| *field == 'C')
                    .map(|(_, value)| value.as_str()),
                Some("0A000"),
                "copy should return an unsupported-feature SQLSTATE"
            );

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/pgwire_extended_execution.rs.
mod pgwire_extended_execution {
    #![allow(unused_imports, dead_code)]

    use super::support_pgwire as pgwire_support;

    use std::net::SocketAddr;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::{DataType, FieldSchema, Schema};
    use pgwire_support::{
        bind_frame, data_dir, describe_statement_frame, execute_frame, parse_data_row, parse_frame,
        parse_frame_with_types, parse_parameter_description, read_until_ready, read_wire_frame,
        startup_frame, sync_frame, use_local_storage,
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

    fn seed_incompatible_parameter_collection(cassie: &Cassie) {
        let collection = canonical_relation_name("postgres", "public", "parameter_type_docs");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: false,
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
                serde_json::json!({"score": 123}),
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

    async fn execute_text_parameter_query(
        reader: &mut PgwireReader<'_>,
        writer: &mut PgwireWriter<'_>,
        name: &str,
        sql: &str,
    ) -> Vec<WireFrame> {
        let portal = format!("portal_{name}");
        tokio::io::AsyncWriteExt::write_all(writer, &parse_frame_with_types(name, sql, &[25]))
            .await
            .expect("write typed parse");
        tokio::io::AsyncWriteExt::write_all(writer, &describe_statement_frame(name))
            .await
            .expect("write typed describe");
        tokio::io::AsyncWriteExt::write_all(writer, &bind_frame(&portal, name, &["123"]))
            .await
            .expect("write text bind");
        tokio::io::AsyncWriteExt::write_all(writer, &execute_frame(&portal))
            .await
            .expect("write typed execute");
        tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
            .await
            .expect("write typed sync");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush typed query");
        read_ready_frames(reader).await
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

    #[test]
    fn should_preserve_unknown_for_incompatible_text_parameters() {
        // Arrange
        use_local_storage();
        let path = data_dir("incompatible-text-parameters");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            seed_incompatible_parameter_collection(&cassie);
            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            start_pgwire_session(&mut reader, &mut write_half).await;

            // Act
            let not_equal = execute_text_parameter_query(
                &mut reader,
                &mut write_half,
                "incompatible_not_equal",
                "SELECT score FROM parameter_type_docs WHERE score != $1",
            )
            .await;
            let equal = execute_text_parameter_query(
                &mut reader,
                &mut write_half,
                "incompatible_equal",
                "SELECT score FROM parameter_type_docs WHERE score = $1",
            )
            .await;
            let less_than = execute_text_parameter_query(
                &mut reader,
                &mut write_half,
                "incompatible_less_than",
                "SELECT score FROM parameter_type_docs WHERE score < $1",
            )
            .await;

            // Assert
            let parameter_description = not_equal
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("parameter description");
            assert_eq!(
                parse_parameter_description(&parameter_description.1),
                vec![25]
            );
            for (operator, frames) in [
                ("!=", not_equal.as_slice()),
                ("=", equal.as_slice()),
                ("<", less_than.as_slice()),
            ] {
                assert!(
                    frames.iter().all(|frame| frame.0 != b'E'),
                    "{operator} query should not return an error"
                );
                assert!(
                    frames.iter().any(|frame| frame.0 == b'C'),
                    "{operator} query should complete"
                );
            }
            assert_eq!(not_equal.iter().filter(|frame| frame.0 == b'D').count(), 0);
            assert_eq!(equal.iter().filter(|frame| frame.0 == b'D').count(), 0);
            assert_eq!(less_than.iter().filter(|frame| frame.0 == b'D').count(), 0);

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}
// Formerly tests/pgwire_extended_lifecycle.rs.
mod pgwire_extended_lifecycle {
    #![allow(unused_imports, dead_code)]

    use super::support_pgwire as pgwire_support;

    use std::net::SocketAddr;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::{DataType, FieldSchema, Schema};
    use pgwire_support::{
        bind_frame, cancel_request_frame, data_dir, describe_statement_frame, execute_frame,
        parse_data_row, parse_error_fields, parse_frame, parse_parameter_description,
        parse_row_description, read_until_ready, read_wire_frame, simple_query_frame,
        startup_frame, sync_frame, use_local_storage,
    };

    type WireFrame = (u8, Vec<u8>);
    type PgwireReader<'a> = tokio::io::BufReader<tokio::net::tcp::ReadHalf<'a>>;
    type PgwireWriter<'a> = tokio::net::tcp::WriteHalf<'a>;
    type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

    fn close_frame(target: u8, name: &str) -> Vec<u8> {
        match target {
            b'S' => pgwire_support::close_statement_frame(name),
            b'P' => pgwire_support::close_portal_frame(name),
            _ => panic!("unsupported close target: {target}"),
        }
    }

    fn frontend_frame(tag: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.push(tag);
        frame.extend_from_slice(
            &i32::try_from(payload.len() + 4)
                .expect("frontend payload size must fit into i32")
                .to_be_bytes(),
        );
        frame.extend_from_slice(payload);
        frame
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

    fn seed_close_cascade_collection(cassie: &Cassie) {
        let collection = canonical_relation_name("postgres", "public", "extended_query_close_docs");
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
        let config = CassieRuntimeConfig::from_env().expect("runtime config");
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
        if auth.1 == 3_i32.to_be_bytes() {
            tokio::io::AsyncWriteExt::write_all(writer, &frontend_frame(b'p', b"postgres\0"))
                .await
                .expect("write password");
            tokio::io::AsyncWriteExt::flush(writer)
                .await
                .expect("flush password");
            let auth_ok = read_wire_frame(reader).await;
            assert_eq!(auth_ok, (b'R', 0_i32.to_be_bytes().to_vec()));
        }
        let startup_ready = read_until_ready(reader).await;
        assert_eq!(startup_ready, vec![b'I']);
    }

    async fn write_close_cascade_batch(writer: &mut PgwireWriter<'_>) {
        tokio::io::AsyncWriteExt::write_all(
            writer,
            &parse_frame(
                "stmt_close_cascade",
                "SELECT title FROM extended_query_close_docs WHERE title = $1 ORDER BY title",
            ),
        )
        .await
        .expect("write parse");
        tokio::io::AsyncWriteExt::write_all(
            writer,
            &bind_frame("portal_close_cascade", "stmt_close_cascade", &["alpha"]),
        )
        .await
        .expect("write bind");
        tokio::io::AsyncWriteExt::write_all(writer, &close_frame(b'S', "stmt_close_cascade"))
            .await
            .expect("write statement close");
        tokio::io::AsyncWriteExt::write_all(writer, &execute_frame("portal_close_cascade"))
            .await
            .expect("write execute");
        tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
            .await
            .expect("write sync");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush close batch");
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

    fn assert_close_cascade_frames(frames: &[WireFrame], cassie: &Cassie) {
        assert_eq!(
            frames.len(),
            5,
            "statement close should remove dependent portals before execute"
        );
        assert_eq!(frames[0].0, b'1');
        assert_eq!(frames[1].0, b'2');
        assert_eq!(frames[2].0, b'3');
        assert_eq!(frames[3].0, b'E');
        assert_eq!(frames[4].0, b'Z');
        assert_eq!(frames[4].1, vec![b'I']);

        let error_fields = parse_error_fields(&frames[3].1);
        assert!(
            error_fields
                .iter()
                .any(|(field, value)| *field == 'M' && value.contains("portal")),
            "execute after statement close should fail because the portal was removed"
        );
        assert!(
            error_fields
                .iter()
                .any(|(field, value)| *field == 'M' && value.contains("not bound")),
            "execute after statement close should mention the missing portal"
        );
        let metrics = cassie.metrics();
        assert_eq!(metrics["pgwire"]["prepared_statements"].as_u64(), Some(0));
        assert_eq!(metrics["pgwire"]["portals"].as_u64(), Some(0));
    }

    #[test]
    fn should_close_statement_cascade_referenced_portals_before_reuse() {
        // Arrange
        use_local_storage();
        let path = data_dir("close_cascade");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = CassieRuntimeConfig::from_env().expect("runtime config");
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            seed_close_cascade_collection(&cassie);

            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            start_pgwire_session(&mut reader, &mut write_half).await;
            write_close_cascade_batch(&mut write_half).await;
            let frames = read_ready_frames(&mut reader).await;

            // Assert
            assert_close_cascade_frames(&frames, &cassie);

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_close_transaction_portals_at_transaction_end() {
        // Arrange
        use_local_storage();
        let path = data_dir("transaction_portal_cleanup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = CassieRuntimeConfig::from_env().expect("runtime config");
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            seed_close_cascade_collection(&cassie);

            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            start_pgwire_session(&mut reader, &mut write_half).await;
            for transaction_end in ["COMMIT", "ROLLBACK"] {
                // Arrange
                tokio::io::AsyncWriteExt::write_all(&mut write_half, &simple_query_frame("BEGIN"))
                    .await
                    .expect("write begin");
                tokio::io::AsyncWriteExt::flush(&mut write_half)
                    .await
                    .expect("flush begin");
                assert_eq!(read_until_ready(&mut reader).await, vec![b'T']);
                let suffix = transaction_end.to_ascii_lowercase();
                let statement = format!("stmt_{suffix}_cleanup");
                let portal = format!("portal_{suffix}_cleanup");
                let mut bind = parse_frame(
                    &statement,
                    "SELECT title FROM extended_query_close_docs WHERE title = $1",
                );
                bind.extend_from_slice(&bind_frame(&portal, &statement, &["alpha"]));
                bind.extend_from_slice(&sync_frame());
                tokio::io::AsyncWriteExt::write_all(&mut write_half, &bind)
                    .await
                    .expect("write portal bind");
                tokio::io::AsyncWriteExt::flush(&mut write_half)
                    .await
                    .expect("flush portal bind");
                assert_eq!(read_until_ready(&mut reader).await, vec![b'T']);
                assert_eq!(cassie.metrics()["pgwire"]["portals"].as_u64(), Some(1));

                // Act
                tokio::io::AsyncWriteExt::write_all(
                    &mut write_half,
                    &simple_query_frame(transaction_end),
                )
                .await
                .expect("write transaction end");
                tokio::io::AsyncWriteExt::flush(&mut write_half)
                    .await
                    .expect("flush transaction end");
                assert_eq!(read_until_ready(&mut reader).await, vec![b'I']);

                // Assert
                assert_eq!(cassie.metrics()["pgwire"]["portals"].as_u64(), Some(0));
            }
            drop(socket);
            server.abort();
        });
    }
}

// Formerly tests/pgwire_extended_metadata.rs.
mod pgwire_extended_metadata {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;

    use super::support_pgwire as wire;
    use wire::*;

    const OID_BOOL: i32 = 16;
    const OID_INT4: i32 = 23;
    const OID_TEXT: i32 = 25;
    const OID_UNKNOWN: i32 = 705;

    #[test]
    fn should_describe_typed_insert_returning_metadata() {
        // Arrange
        use_local_storage();
        let path = data_dir("typed_insert_returning");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE pgwire_typed_items (id TEXT, score INT, active BOOLEAN)",
                vec![],
            )
            .unwrap();
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        complete_startup(&mut reader, &mut write_half).await;

        // Act
        write_frames(
            &mut write_half,
            vec![
                parse_frame_with_types(
                    "typed_insert",
                    "INSERT INTO pgwire_typed_items (id, score, active) VALUES ($1, $2, $3) RETURNING id, score, active",
                    &[OID_TEXT, OID_INT4, OID_BOOL],
                ),
                describe_statement_frame("typed_insert"),
                bind_frame("typed_insert_portal", "typed_insert", &["typed-1", "42", "true"]),
                execute_frame("typed_insert_portal"),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'1', b't', b'T', b'2', b'D', b'C', b'Z']
        );
        assert_eq!(
            parse_parameter_description(&frames[1].1),
            vec![OID_TEXT, OID_INT4, OID_BOOL]
        );
        let columns = parse_row_description(&frames[2].1);
        assert_eq!(
            columns
                .iter()
                .map(|column| (column.name.as_str(), column.type_oid))
                .collect::<Vec<_>>(),
            vec![("id", OID_TEXT), ("score", OID_INT4), ("active", OID_BOOL)]
        );
        assert_eq!(
            parse_data_row(&frames[4].1),
            vec![
                Some("typed-1".to_string()),
                Some("42".to_string()),
                Some("true".to_string())
            ]
        );
        assert_eq!(frames[6].1, vec![b'I']);

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_infer_parameter_metadata_for_crud_returning_flows() {
        // Arrange
        use_local_storage();
        let path = data_dir("infer_crud_metadata");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE pgwire_infer_items (label TEXT, score INT, active BOOLEAN)",
                    vec![],
                )
                .unwrap();
            cassie
            .execute_sql(
                &session,
                "INSERT INTO pgwire_infer_items (label, score, active) VALUES ('item-1', 7, true)",
                vec![],
            )
            .unwrap();
            let server = spawn_server(cassie.clone()).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            complete_startup(&mut reader, &mut write_half).await;

            // Act
            write_frames(
                &mut write_half,
                vec![
                    parse_frame(
                        "infer_insert",
                        "INSERT INTO pgwire_infer_items (label) VALUES ($1) RETURNING label",
                    ),
                    describe_statement_frame("infer_insert"),
                    parse_frame(
                        "infer_select",
                        "SELECT label FROM pgwire_infer_items WHERE score = $1 AND active = $2",
                    ),
                    describe_statement_frame("infer_select"),
                    parse_frame(
                        "infer_update",
                        "UPDATE pgwire_infer_items SET score = $1 WHERE label = $2 RETURNING score",
                    ),
                    describe_statement_frame("infer_update"),
                    parse_frame(
                        "infer_delete",
                        "DELETE FROM pgwire_infer_items WHERE label = $1 RETURNING label",
                    ),
                    describe_statement_frame("infer_delete"),
                    sync_frame(),
                ],
            )
            .await;
            let frames = read_frames_until_ready(&mut reader).await;

            // Assert
            assert_eq!(
                frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
                vec![b'1', b't', b'T', b'1', b't', b'T', b'1', b't', b'T', b'1', b't', b'T', b'Z']
            );
            assert_eq!(parse_parameter_description(&frames[1].1), vec![OID_TEXT]);
            assert_eq!(
                parse_parameter_description(&frames[4].1),
                vec![OID_INT4, OID_BOOL]
            );
            assert_eq!(
                parse_parameter_description(&frames[7].1),
                vec![OID_INT4, OID_TEXT]
            );
            assert_eq!(parse_parameter_description(&frames[10].1), vec![OID_TEXT]);
            assert_eq!(parse_row_description(&frames[2].1)[0].type_oid, OID_TEXT);
            assert_eq!(parse_row_description(&frames[5].1)[0].type_oid, OID_TEXT);
            assert_eq!(parse_row_description(&frames[8].1)[0].type_oid, OID_INT4);
            assert_eq!(parse_row_description(&frames[11].1)[0].type_oid, OID_TEXT);
            assert_eq!(frames[12].1, vec![b'I']);

            drop(socket);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_infer_case_result_parameter_types_from_sibling_branches() {
        // Arrange
        use_local_storage();
        let path = data_dir("infer_case_parameters");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            for statement in [
                "CREATE TABLE pgwire_case_items (label TEXT, score INT)",
                "INSERT INTO pgwire_case_items (label, score) VALUES ('item-7', 7)",
                "INSERT INTO pgwire_case_items (label, score) VALUES ('item-0', 0)",
            ] {
                cassie
                    .execute_sql(&session, statement, vec![])
                    .expect(statement);
            }
            let server = spawn_server(cassie.clone()).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            complete_startup(&mut reader, &mut write_half).await;

            // Act
            write_frames(
                &mut write_half,
                vec![
                    parse_frame(
                        "case_params",
                        "SELECT CASE WHEN score > 1 THEN $1 ELSE score END AS bucket, CASE WHEN score > 1 THEN label ELSE $2 END AS named FROM pgwire_case_items ORDER BY score",
                    ),
                    describe_statement_frame("case_params"),
                    bind_frame("case_params_portal", "case_params", &["100", "low"]),
                    execute_frame("case_params_portal"),
                    sync_frame(),
                ],
            )
            .await;
            let frames = read_frames_until_ready(&mut reader).await;

            // Assert
            assert_eq!(
                frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
                vec![b'1', b't', b'T', b'2', b'D', b'D', b'C', b'Z']
            );
            assert_eq!(
                parse_parameter_description(&frames[1].1),
                vec![OID_INT4, OID_TEXT]
            );
            assert_eq!(
                parse_row_description(&frames[2].1)
                    .iter()
                    .map(|column| column.type_oid)
                    .collect::<Vec<_>>(),
                vec![OID_INT4, OID_TEXT]
            );
            assert_eq!(
                vec![parse_data_row(&frames[4].1), parse_data_row(&frames[5].1)],
                vec![
                    vec![Some("0".to_string()), Some("low".to_string())],
                    vec![Some("100".to_string()), Some("item-7".to_string())],
                ]
            );

            drop(socket);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reuse_unnamed_statement_metadata_lifecycle() {
        // Arrange
        use_local_storage();
        let path = data_dir("unnamed_metadata");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE pgwire_unnamed_items (label TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO pgwire_unnamed_items (label, score) VALUES ('item-1', 5)",
                    vec![],
                )
                .unwrap();
            let server = spawn_server(cassie.clone()).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            complete_startup(&mut reader, &mut write_half).await;

            // Act
            write_frames(
                &mut write_half,
                vec![
                    parse_frame(
                        "",
                        "SELECT label FROM pgwire_unnamed_items WHERE score = $1 ORDER BY label",
                    ),
                    describe_statement_frame(""),
                    bind_frame("", "", &["5"]),
                    describe_portal_frame(""),
                    execute_frame(""),
                    close_portal_frame(""),
                    close_statement_frame(""),
                    sync_frame(),
                ],
            )
            .await;
            let frames = read_frames_until_ready(&mut reader).await;

            // Assert
            assert_eq!(
                frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
                vec![b'1', b't', b'T', b'2', b'T', b'D', b'C', b'3', b'3', b'Z']
            );
            assert_eq!(parse_parameter_description(&frames[1].1), vec![OID_INT4]);
            assert_eq!(parse_row_description(&frames[2].1)[0].type_oid, OID_TEXT);
            assert_eq!(parse_row_description(&frames[4].1)[0].type_oid, OID_TEXT);
            assert_eq!(
                parse_data_row(&frames[5].1),
                vec![Some("item-1".to_string())]
            );
            assert_eq!(frames[9].1, vec![b'I']);

            drop(socket);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_preserve_table_free_parameter_oids_through_describe() {
        // Arrange
        use_local_storage();
        let path = data_dir("table_free_parameter_metadata");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let server = spawn_server(cassie.clone()).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            complete_startup(&mut reader, &mut write_half).await;

            // Act
            write_frames(
                &mut write_half,
                vec![
                    parse_frame("table_free_inferred", "SELECT $1::INT AS value"),
                    describe_statement_frame("table_free_inferred"),
                    parse_frame_with_types(
                        "table_free_explicit",
                        "SELECT $1 AS value",
                        &[OID_BOOL],
                    ),
                    describe_statement_frame("table_free_explicit"),
                    sync_frame(),
                ],
            )
            .await;
            let frames = read_frames_until_ready(&mut reader).await;

            // Assert
            assert_eq!(
                frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
                vec![b'1', b't', b'T', b'1', b't', b'T', b'Z']
            );
            assert_eq!(parse_parameter_description(&frames[1].1), vec![OID_INT4]);
            assert_eq!(parse_row_description(&frames[2].1)[0].type_oid, OID_INT4);
            assert_eq!(parse_parameter_description(&frames[4].1), vec![OID_BOOL]);
            assert_eq!(parse_row_description(&frames[5].1)[0].type_oid, OID_BOOL);

            drop(socket);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_recover_ready_state_after_extended_statement_error() {
        // Arrange
        use_local_storage();
        let path = data_dir("extended_error_recovery");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE pgwire_recovery_items (label TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO pgwire_recovery_items (label, score) VALUES ('item-1', 9)",
                    vec![],
                )
                .unwrap();
            let server = spawn_server(cassie.clone()).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            complete_startup(&mut reader, &mut write_half).await;

            // Act
            write_frames(
                &mut write_half,
                vec![
                    parse_frame(
                        "recovery_stmt",
                        "SELECT label FROM pgwire_recovery_items WHERE score = $1",
                    ),
                    bind_frame("bad_portal", "recovery_stmt", &["9", "extra"]),
                    parse_frame(
                        "ignored_stmt",
                        "SELECT label FROM pgwire_recovery_items WHERE score = $1",
                    ),
                    bind_frame("ignored_portal", "ignored_stmt", &["9"]),
                    execute_frame("ignored_portal"),
                    sync_frame(),
                ],
            )
            .await;
            let error_frames = read_frames_until_ready(&mut reader).await;
            write_frames(
                &mut write_half,
                vec![
                    bind_frame("good_portal", "recovery_stmt", &["9"]),
                    execute_frame("good_portal"),
                    sync_frame(),
                ],
            )
            .await;
            let recovery_frames = read_frames_until_ready(&mut reader).await;

            // Assert
            assert_eq!(
                error_frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
                vec![b'1', b'E', b'Z']
            );
            let fields = parse_error_fields(&error_frames[1].1);
            assert!(fields
                .iter()
                .any(|(field, value)| *field == 'S' && value == "ERROR"));
            assert!(fields
                .iter()
                .any(|(field, value)| *field == 'C' && value == "08P01"));
            assert!(fields
                .iter()
                .any(|(field, value)| *field == 'M' && value.contains("requires 1")));
            assert_eq!(error_frames[2].1, vec![b'I']);

            assert_eq!(
                recovery_frames
                    .iter()
                    .map(|frame| frame.0)
                    .collect::<Vec<_>>(),
                vec![b'2', b'T', b'D', b'C', b'Z']
            );
            assert_eq!(
                parse_data_row(&recovery_frames[2].1),
                vec![Some("item-1".to_string())]
            );
            assert_eq!(recovery_frames[4].1, vec![b'I']);

            drop(socket);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_describe_builtin_function_results_with_runtime_types() {
        // Arrange
        const OID_INT8: i32 = 20;
        const OID_FLOAT8: i32 = 701;
        use_local_storage();
        let path = data_dir("builtin_function_result_types");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let cases = [
            ("SELECT length(name) FROM function_result_items", OID_INT4),
            ("SELECT abs(score) FROM function_result_items", OID_INT4),
            ("SELECT abs(ratio) FROM function_result_items", OID_FLOAT8),
            (
                "SELECT coalesce(score, 0) FROM function_result_items",
                OID_INT4,
            ),
            (
                "SELECT pg_backend_pid() FROM function_result_items",
                OID_INT4,
            ),
            (
                "SELECT has_database_privilege('cassie', 'CONNECT') FROM function_result_items",
                OID_BOOL,
            ),
            ("SELECT min(wide) FROM function_result_items", OID_INT8),
            ("SELECT max(score) FROM function_result_items", OID_INT4),
            ("SELECT max(name) FROM function_result_items", OID_TEXT),
            ("SELECT count(name) FROM function_result_items", OID_INT8),
            ("SELECT sum(score) FROM function_result_items", OID_INT8),
            ("SELECT sum(ratio) FROM function_result_items", OID_FLOAT8),
        ];

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE function_result_items (name TEXT, score INT, wide BIGINT, ratio FLOAT)",
                    vec![],
                )
                .unwrap();
            let server = spawn_server(cassie.clone()).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            complete_startup(&mut reader, &mut write_half).await;

            // Act
            let mut described = Vec::with_capacity(cases.len());
            for (sql, _) in cases {
                write_frames(
                    &mut write_half,
                    vec![
                        parse_frame("", sql),
                        describe_statement_frame(""),
                        sync_frame(),
                    ],
                )
                .await;
                let frames = read_frames_until_ready(&mut reader).await;
                let columns = frames.iter().find(|frame| frame.0 == b'T').map_or_else(
                    || panic!("row description for {sql}: {frames:?}"),
                    |frame| parse_row_description(&frame.1),
                );
                described.push((sql, columns[0].type_oid));
            }

            // Assert
            assert_eq!(described, cases.to_vec());

            drop(socket);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/pgwire_extended_prepared.rs.
mod pgwire_extended_prepared {
    #![allow(unused_imports, dead_code)]
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::{DataType, FieldSchema, Schema};

    use super::support_pgwire as pgwire_support;

    use pgwire_support::{
        bind_frame, cancel_request_frame, data_dir, describe_statement_frame, execute_frame,
        parse_data_row, parse_error_fields, parse_frame, parse_parameter_description,
        read_frames_until_ready, read_until_ready, read_wire_frame, startup_frame, sync_frame,
        use_local_storage,
    };

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

    fn score_schema() -> Schema {
        Schema {
            fields: vec![FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            }],
        }
    }

    fn seed_score_collection(cassie: &Cassie, collection: &str) {
        let collection = canonical_relation_name("postgres", "public", collection);
        let schema = score_schema();
        cassie
            .midge
            .create_collection(&collection, schema.clone())
            .unwrap();
        cassie.register_collection(&collection, schema);
        for (id, score) in [("doc-1", 1), ("doc-2", 2)] {
            cassie
                .midge
                .put_document(
                    &collection,
                    Some(id.to_string()),
                    serde_json::json!({"score": score}),
                )
                .unwrap();
        }
    }

    async fn spawn_pgwire_server(
        cassie: &Cassie,
    ) -> (
        std::net::SocketAddr,
        tokio::task::JoinHandle<Result<(), cassie::CassieError>>,
    ) {
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

    async fn connect_authenticated_pgwire(
        addr: std::net::SocketAddr,
    ) -> (
        tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
        tokio::net::tcp::OwnedWriteHalf,
    ) {
        let socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = tokio::io::BufReader::new(read_half);
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup_frame("root", "postgres"))
            .await
            .expect("write startup");
        let auth = read_wire_frame(&mut reader).await;
        assert_eq!(auth.0, b'R', "startup should return an auth response");
        assert_eq!(
            i32::from_be_bytes(auth.1[0..4].try_into().expect("auth payload")),
            3,
            "startup should request a cleartext password"
        );
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &password_frame("postgres"))
            .await
            .expect("write password");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush password");
        let auth_ok = read_wire_frame(&mut reader).await;
        assert_eq!(auth_ok.0, b'R', "password should return an auth response");
        assert_eq!(
            i32::from_be_bytes(auth_ok.1[0..4].try_into().expect("auth payload")),
            0,
            "password auth should succeed"
        );
        let startup_ready = read_until_ready(&mut reader).await;
        assert_eq!(startup_ready, vec![b'I']);
        (reader, write_half)
    }

    async fn execute_reused_statement(
        writer: &mut tokio::net::tcp::OwnedWriteHalf,
        statement_name: &str,
        sql: &str,
        portals: [(&str, &str); 2],
    ) {
        tokio::io::AsyncWriteExt::write_all(writer, &parse_frame(statement_name, sql))
            .await
            .expect("write parse");
        for (portal_name, param) in portals {
            tokio::io::AsyncWriteExt::write_all(
                writer,
                &bind_frame(portal_name, statement_name, &[param]),
            )
            .await
            .expect("write bind");
            tokio::io::AsyncWriteExt::write_all(writer, &execute_frame(portal_name))
                .await
                .expect("write execute");
        }
        tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
            .await
            .expect("write sync");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush frames");
    }

    fn assert_reused_statement_frames(frames: &[(u8, Vec<u8>)]) {
        assert_eq!(
            frames.len(),
            10,
            "reused prepared statements should return ten frames"
        );
        assert_eq!(frames[0].0, b'1', "parse should complete first");
        assert_eq!(frames[1].0, b'2', "first bind should complete");
        assert_eq!(frames[2].0, b'T', "first execute should describe rows");
        assert_eq!(frames[3].0, b'D', "first execute should return a data row");
        assert_eq!(
            frames[4].0, b'C',
            "first execute should finish with command complete"
        );
        assert_eq!(
            frames[5].0, b'2',
            "second bind should reuse the prepared statement"
        );
        assert_eq!(frames[6].0, b'T', "second execute should describe rows");
        assert_eq!(frames[7].0, b'D', "second execute should return a data row");
        assert_eq!(
            frames[8].0, b'C',
            "second execute should finish with command complete"
        );
        assert_eq!(frames[9].0, b'Z', "sync should finish with ready-for-query");
    }

    async fn shutdown_pgwire_server(
        server: tokio::task::JoinHandle<Result<(), cassie::CassieError>>,
    ) {
        server.abort();
        let _ = server.await;
    }

    #[test]
    fn should_reuse_prepared_statement_for_binary_extended_query_bindings() {
        // Arrange
        use_local_storage();
        let path = data_dir("reuse");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            seed_score_collection(&cassie, "extended_query_numbers");
            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let (mut reader, mut write_half) = connect_authenticated_pgwire(addr).await;

            // Act
            execute_reused_statement(
                &mut write_half,
                "stmt_extended_reuse",
                "SELECT score FROM extended_query_numbers WHERE score = $1 ORDER BY score",
                [("portal_one", "1"), ("portal_two", "2")],
            )
            .await;
            let frames = read_frames_until_ready(&mut reader).await;

            // Assert
            assert_reused_statement_frames(&frames);
            let first_values = parse_data_row(&frames[3].1);
            let second_values = parse_data_row(&frames[7].1);
            assert_eq!(first_values, vec![Some("1".to_string())]);
            assert_eq!(second_values, vec![Some("2".to_string())]);

            drop(write_half);
            shutdown_pgwire_server(server).await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_parse_prepared_statement_once_across_repeated_extended_executes() {
        // Arrange
        use_local_storage();
        let path = data_dir("parse_once");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            seed_score_collection(&cassie, "extended_query_parse_once");
            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let (mut reader, mut write_half) = connect_authenticated_pgwire(addr).await;

            // Act
            execute_reused_statement(
                &mut write_half,
                "stmt_extended_parse_once",
                "SELECT score FROM extended_query_parse_once WHERE score = $1 ORDER BY score",
                [
                    ("portal_parse_once_one", "1"),
                    ("portal_parse_once_two", "2"),
                ],
            )
            .await;
            let _ = read_frames_until_ready(&mut reader).await;
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(metrics["runtime"]["sql_parse_total"].as_u64(), Some(1));
            assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
            assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));

            drop(write_half);
            shutdown_pgwire_server(server).await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/pgwire_portal_safety.rs.
mod pgwire_portal_safety {
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use tokio::io::BufReader;

    use super::support_pgwire as support;

    fn configured_cassie(
        label: &str,
        max_result_rows: usize,
    ) -> (Cassie, CassieRuntimeConfig, String) {
        configured_cassie_with_memory(
            label,
            max_result_rows,
            CassieRuntimeConfig::from_env()
                .expect("runtime config")
                .limits
                .query_memory_budget_bytes,
        )
    }

    fn configured_cassie_with_memory(
        label: &str,
        max_result_rows: usize,
        query_memory_budget_bytes: usize,
    ) -> (Cassie, CassieRuntimeConfig, String) {
        support::use_local_storage();
        let path = support::data_dir(label);
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.max_result_rows = max_result_rows;
        config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
        cassie.startup().expect("startup");
        (cassie, config, path)
    }

    fn seed_large_rows(cassie: &Cassie, table: &str, count: usize, payload_size: usize) {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {table} (payload TEXT)"),
                vec![],
            )
            .expect("create table");
        let rows = (0..count)
            .map(|index| {
                (
                    Some(format!("doc-{index:04}")),
                    serde_json::json!({
                        "payload": format!("{index:04}-{}", "x".repeat(payload_size)),
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(table, rows)
            .expect("seed rows");
    }

    fn seed_rows(cassie: &Cassie, table: &str, count: usize) {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {table} (payload TEXT)"),
                vec![],
            )
            .expect("create table");
        for index in 0..count {
            cassie
                .midge
                .put_document(
                    table,
                    Some(format!("doc-{index:04}")),
                    serde_json::json!({"payload": format!("value-{index:04}")}),
                )
                .expect("seed row");
        }
    }

    async fn spawn_server(
        cassie: Cassie,
        config: CassieRuntimeConfig,
    ) -> (
        std::net::SocketAddr,
        tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            address.to_string(),
            Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        (address, server)
    }

    fn data_values(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'D')
            .filter_map(|(_, payload)| {
                support::parse_data_row(payload)
                    .into_iter()
                    .next()
                    .flatten()
            })
            .collect()
    }

    fn error_code(frames: &[(u8, Vec<u8>)]) -> Option<String> {
        let (_, payload) = frames.iter().find(|(tag, _)| *tag == b'E')?;
        support::parse_error_fields(payload)
            .into_iter()
            .find_map(|(tag, value)| (tag == 'C').then_some(value))
    }

    #[test]
    fn should_execute_i32_max_portal_page_without_capacity_sized_allocation() {
        // Arrange
        let (cassie, config, path) = configured_cassie("portal-i32-max", 16);
        seed_rows(&cassie, "portal_i32_max", 2);
        let observed = cassie.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (address, server) = spawn_server(cassie, config).await;
            let mut socket = tokio::net::TcpStream::connect(address)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            let before = observed.midge.query_scan_entries_for_diagnostics();

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame("max_stmt", "SELECT payload FROM portal_i32_max"),
                    support::bind_frame("max_portal", "max_stmt", &[]),
                    support::execute_limited_frame("max_portal", i32::MAX),
                    support::sync_frame(),
                ],
            )
            .await;
            let frames = support::read_frames_until_ready(&mut reader).await;
            let visited = observed
                .midge
                .query_scan_entries_for_diagnostics()
                .saturating_sub(before);

            // Assert
            assert_eq!(data_values(&frames).len(), 2);
            assert!(!frames.iter().any(|(tag, _)| *tag == b's'));
            assert_eq!(visited, 2);

            drop(socket);
            server.abort();
            let _ = server.await;
        });
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_enforce_result_row_cap_cumulatively_across_portal_resumes() {
        // Arrange
        let (cassie, config, path) = configured_cassie("portal-cumulative-cap", 3);
        seed_rows(&cassie, "portal_cumulative_cap", 5);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (address, server) = spawn_server(cassie, config).await;
            let mut socket = tokio::net::TcpStream::connect(address)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame("cap_stmt", "SELECT payload FROM portal_cumulative_cap"),
                    support::bind_frame("cap_portal", "cap_stmt", &[]),
                    support::execute_limited_frame("cap_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;
            let first = support::read_frames_until_ready(&mut reader).await;
            assert_eq!(data_values(&first).len(), 2);
            assert!(first.iter().any(|(tag, _)| *tag == b's'));

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::execute_limited_frame("cap_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;
            let overflow = support::read_frames_until_ready(&mut reader).await;

            // Assert
            assert_eq!(error_code(&overflow).as_deref(), Some("54000"));
            assert_eq!(
                data_values(&overflow).len(),
                0,
                "overflow page must be atomic"
            );

            drop(socket);
            server.abort();
            let _ = server.await;
        });
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_use_transaction_overlay_for_streaming_portal_results() {
        // Arrange
        let (cassie, config, path) = configured_cassie("portal-overlay", 16);
        seed_rows(&cassie, "portal_overlay", 1);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (address, server) = spawn_server(cassie, config).await;
            let mut socket = tokio::net::TcpStream::connect(address)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            support::write_frames(&mut write_half, vec![support::simple_query_frame("BEGIN")])
                .await;
            let _ = support::read_frames_until_ready(&mut reader).await;
            support::write_frames(
                &mut write_half,
                vec![support::simple_query_frame(
                    "INSERT INTO portal_overlay (payload) VALUES ('staged')",
                )],
            )
            .await;
            let inserted = support::read_frames_until_ready(&mut reader).await;
            assert!(inserted.iter().all(|(tag, _)| *tag != b'E'));

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame("overlay_stmt", "SELECT payload FROM portal_overlay"),
                    support::bind_frame("overlay_portal", "overlay_stmt", &[]),
                    support::execute_limited_frame("overlay_portal", 8),
                    support::sync_frame(),
                ],
            )
            .await;
            let frames = support::read_frames_until_ready(&mut reader).await;
            let values = data_values(&frames);

            // Assert
            assert_eq!(values.len(), 2);
            assert!(values.iter().any(|value| value == "staged"));

            drop(socket);
            server.abort();
            let _ = server.await;
        });
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_enforce_retained_memory_budget_across_named_portal_lifecycle() {
        // Arrange
        let (cassie, config, path) =
            configured_cassie_with_memory("portal-shared-memory", 1_000, 40 * 1_024);
        seed_large_rows(&cassie, "portal_shared_memory", 64, 128);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let (address, server) = spawn_server(cassie, config).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame(
                    "memory_stmt",
                    "SELECT lower(payload) AS payload FROM portal_shared_memory WHERE payload IS NOT NULL",
                ),
                support::bind_frame("memory_portal_one", "memory_stmt", &[]),
                support::execute_limited_frame("memory_portal_one", 1),
                support::sync_frame(),
            ],
        )
        .await;
        let first = support::read_frames_until_ready(&mut reader).await;
        assert_eq!(
            data_values(&first).len(),
            1,
            "first portal frames: {first:?}"
        );
        assert!(first.iter().all(|(tag, _)| *tag != b'E'));

        support::write_frames(
            &mut write_half,
            vec![
                support::bind_frame("memory_portal_two", "memory_stmt", &[]),
                support::execute_limited_frame("memory_portal_two", 1),
                support::sync_frame(),
            ],
        )
        .await;
        let second = support::read_frames_until_ready(&mut reader).await;
        assert_eq!(data_values(&second).len(), 1);
        assert!(second.iter().all(|(tag, _)| *tag != b'E'));

        support::write_frames(
            &mut write_half,
            vec![
                support::bind_frame("memory_portal_three", "memory_stmt", &[]),
                support::execute_limited_frame("memory_portal_three", 1),
                support::sync_frame(),
            ],
        )
        .await;
        let third = support::read_frames_until_ready(&mut reader).await;
        assert_eq!(data_values(&third).len(), 1);
        assert!(third.iter().all(|(tag, _)| *tag != b'E'));

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::bind_frame("memory_portal_four", "memory_stmt", &[]),
                support::execute_limited_frame("memory_portal_four", 1),
                support::sync_frame(),
            ],
        )
        .await;
        let overflow = support::read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(error_code(&overflow).as_deref(), Some("54000"));
        assert!(data_values(&overflow).is_empty());

        support::write_frames(
            &mut write_half,
            vec![
                support::close_portal_frame("memory_portal_one"),
                support::bind_frame("memory_portal_five", "memory_stmt", &[]),
                support::execute_limited_frame("memory_portal_five", 1),
                support::sync_frame(),
            ],
        )
        .await;
        let after_close = support::read_frames_until_ready(&mut reader).await;
        assert_eq!(data_values(&after_close).len(), 1);
        assert!(after_close.iter().all(|(tag, _)| *tag != b'E'));

        drop(socket);
        server.abort();
        let _ = server.await;
    });
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/pgwire_portal_streaming.rs.
mod pgwire_portal_streaming {
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use tokio::io::{AsyncWriteExt, BufReader};

    use super::support_pgwire as support;

    #[test]
    fn should_stop_portal_scan_after_requested_page_is_buffered() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("portal-streaming-page");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE portal_streaming_page (id TEXT, payload TEXT)",
                    vec![],
                )
                .expect("create table");
            for index in 0..50 {
                cassie
                    .midge
                    .put_document(
                        "portal_streaming_page",
                        Some(format!("doc-{index:02}")),
                        serde_json::json!({"id": format!("doc-{index:02}"), "payload": "value"}),
                    )
                    .expect("seed row");
            }
            let observed = cassie.clone();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(cassie::pgwire::server::run(
                addr.to_string(),
                Arc::new(cassie),
                config,
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("query connection");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            let before = observed.midge.query_scan_entries_for_diagnostics();

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame(
                        "streaming_stmt",
                        "SELECT payload FROM portal_streaming_page",
                    ),
                    support::bind_frame("streaming_portal", "streaming_stmt", &[]),
                    support::execute_limited_frame("streaming_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;
            let frames = support::read_frames_until_ready(&mut reader).await;
            let visited = observed
                .midge
                .query_scan_entries_for_diagnostics()
                .saturating_sub(before);

            // Assert
            assert!(frames.iter().any(|(tag, _)| *tag == b's'));
            assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'D').count(), 2);
            assert_eq!(visited, 3, "portal page should buffer one lookahead row");

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_cancel_suspended_portal_before_resume() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("portal-suspended-cancel");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE portal_suspended_cancel (id TEXT, payload TEXT)",
                    vec![],
                )
                .expect("create table");
            for index in 0..5 {
                cassie
                    .midge
                    .put_document(
                        "portal_suspended_cancel",
                        Some(format!("doc-{index:02}")),
                        serde_json::json!({"id": format!("doc-{index:02}"), "payload": "value"}),
                    )
                    .expect("seed row");
            }
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(cassie::pgwire::server::run(
                addr.to_string(),
                Arc::new(cassie),
                config,
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("query connection");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            let (process_id, secret_key) =
                support::complete_startup_with_backend_key(&mut reader, &mut write_half).await;
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame(
                        "cancel_stmt",
                        "SELECT payload FROM portal_suspended_cancel",
                    ),
                    support::bind_frame("cancel_portal", "cancel_stmt", &[]),
                    support::execute_limited_frame("cancel_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;
            let initial = support::read_frames_until_ready(&mut reader).await;
            assert!(initial.iter().any(|(tag, _)| *tag == b's'));

            // Act
            let mut cancel_socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("cancel connection");
            cancel_socket
                .write_all(&support::cancel_request_frame(process_id, secret_key))
                .await
                .expect("write cancel request");
            cancel_socket
                .shutdown()
                .await
                .expect("close cancel request");
            tokio::time::sleep(Duration::from_millis(50)).await;
            support::write_frames(
                &mut write_half,
                vec![
                    support::execute_limited_frame("cancel_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;
            let resumed = support::read_frames_until_ready(&mut reader).await;

            // Assert
            let error = resumed
                .iter()
                .find(|(tag, _)| *tag == b'E')
                .expect("suspended portal cancellation error");
            let fields = support::parse_error_fields(&error.1);
            assert!(fields
                .iter()
                .any(|(tag, value)| *tag == 'C' && value == "57014"));

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_resume_portal_from_original_snapshot_after_concurrent_insert() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("portal-snapshot-resume");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE portal_snapshot_rows (value TEXT)",
                    vec![],
                )
                .expect("create table");
            for id in ["doc-a", "doc-b", "doc-c"] {
                cassie
                    .midge
                    .put_document(
                        "portal_snapshot_rows",
                        Some(id.to_string()),
                        serde_json::json!({"value": id}),
                    )
                    .expect("seed row");
            }
            let observed = cassie.clone();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(cassie::pgwire::server::run(
                addr.to_string(),
                Arc::new(cassie),
                config,
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("query connection");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame("snapshot_stmt", "SELECT id FROM portal_snapshot_rows"),
                    support::bind_frame("snapshot_portal", "snapshot_stmt", &[]),
                    support::execute_limited_frame("snapshot_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;
            let initial = support::read_frames_until_ready(&mut reader).await;
            assert_eq!(
                data_values(&initial),
                vec!["doc-a".to_string(), "doc-b".to_string()]
            );
            observed
                .midge
                .put_document(
                    "portal_snapshot_rows",
                    Some("doc-aa".to_string()),
                    serde_json::json!({"value": "doc-aa"}),
                )
                .expect("concurrent insert");

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::execute_limited_frame("snapshot_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;
            let resumed = support::read_frames_until_ready(&mut reader).await;

            // Assert
            assert_eq!(data_values(&resumed), vec!["doc-c".to_string()]);
            assert!(!resumed.iter().any(|(tag, _)| *tag == b's'));

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_align_extended_wildcard_data_rows_with_row_description_when_table_declares_id() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("extended-wildcard-declared-id");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (cassie, row_id) = declared_id_table(&path, "extended_wildcard_id_docs");
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("query connection");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame(
                        "wildcard_stmt",
                        "SELECT * FROM extended_wildcard_id_docs",
                    ),
                    support::describe_statement_frame("wildcard_stmt"),
                    support::bind_frame("wildcard_portal", "wildcard_stmt", &[]),
                    support::execute_frame("wildcard_portal"),
                    support::sync_frame(),
                ],
            )
            .await;
            let frames = support::read_frames_until_ready(&mut reader).await;

            // Assert
            assert!(frames.iter().all(|(tag, _)| *tag != b'E'));
            assert_eq!(
                support::row_description_names(&frames),
                vec!["id".to_string(), "title".to_string()]
            );
            assert_eq!(
                support::data_rows(&frames),
                vec![vec![Some(row_id), Some("x".to_string())]]
            );

            drop(socket);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_align_streaming_portal_wildcard_data_rows_with_row_description_when_table_declares_id(
    ) {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("portal-wildcard-declared-id");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (cassie, row_id) = declared_id_table(&path, "portal_wildcard_id_docs");
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("query connection");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame("wildcard_stmt", "SELECT * FROM portal_wildcard_id_docs"),
                    support::bind_frame("wildcard_portal", "wildcard_stmt", &[]),
                    support::describe_portal_frame("wildcard_portal"),
                    support::execute_limited_frame("wildcard_portal", 10),
                    support::sync_frame(),
                ],
            )
            .await;
            let frames = support::read_frames_until_ready(&mut reader).await;

            // Assert
            assert!(frames.iter().all(|(tag, _)| *tag != b'E'));
            assert_eq!(
                support::row_description_names(&frames),
                vec!["id".to_string(), "title".to_string()]
            );
            assert_eq!(
                support::data_rows(&frames),
                vec![vec![Some(row_id), Some("x".to_string())]]
            );

            drop(socket);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    /// Seeds `count` rows of `{"payload": "value-NN"}` straight through the
    /// storage adapter, bypassing SQL so the test only exercises paging.
    fn seed_payload_rows(cassie: &Cassie, collection: &str, count: usize) {
        for index in 0..count {
            cassie
                .midge
                .put_document(
                    collection,
                    Some(format!("doc-{index:02}")),
                    serde_json::json!({"payload": format!("value-{index:02}")}),
                )
                .expect("seed row");
        }
    }

    /// Runs simple queries on an established connection, asserting none error.
    async fn run_simple_queries(
        reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
        write_half: &mut tokio::net::tcp::WriteHalf<'_>,
        statements: &[&str],
    ) {
        for sql in statements {
            support::write_frames(write_half, vec![support::simple_query_frame(sql)]).await;
            let frames = support::read_frames_until_ready(reader).await;
            assert!(frames.iter().all(|(tag, _)| *tag != b'E'), "{sql} failed");
        }
    }

    /// Commits `sql` from a separate connection so the suspended portal under
    /// test sees a concurrent write land between two Execute messages.
    async fn commit_delete_from_second_connection(addr: std::net::SocketAddr, sql: &str) {
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("writer connection");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        support::write_frames(&mut write_half, vec![support::simple_query_frame(sql)]).await;
        let frames = support::read_frames_until_ready(&mut reader).await;
        assert!(frames.iter().all(|(tag, _)| *tag != b'E'), "delete failed");
    }

    #[test]
    fn should_page_portal_from_one_result_when_delete_commits_during_staged_transaction() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("portal-staged-concurrent-delete");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
            config.limits.parallel_scan_workers = 1;
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE portal_staged_rows (payload TEXT)",
                    vec![],
                )
                .expect("create table");
            seed_payload_rows(&cassie, "portal_staged_rows", 6);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(cassie::pgwire::server::run(
                addr.to_string(),
                Arc::new(cassie),
                config,
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut reader_socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("reader connection");
            let (read_half, mut write_half) = reader_socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            run_simple_queries(
                &mut reader,
                &mut write_half,
                &["BEGIN", "INSERT INTO portal_staged_rows (payload) VALUES ('staged')"],
            )
            .await;
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame("staged_stmt", "SELECT payload FROM portal_staged_rows"),
                    support::bind_frame("staged_portal", "staged_stmt", &[]),
                    support::execute_limited_frame("staged_portal", 3),
                    support::sync_frame(),
                ],
            )
            .await;
            let first_page = data_values(&support::read_frames_until_ready(&mut reader).await);
            commit_delete_from_second_connection(
                addr,
                "DELETE FROM portal_staged_rows WHERE payload = 'value-00'",
            )
            .await;

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::execute_limited_frame("staged_portal", 100),
                    support::sync_frame(),
                ],
            )
            .await;
            let second_page = data_values(&support::read_frames_until_ready(&mut reader).await);

            // Assert
            assert_eq!(first_page.len(), 3, "first page = {first_page:?}");
            let mut all_pages = first_page
                .iter()
                .chain(second_page.iter())
                .cloned()
                .collect::<Vec<_>>();
            all_pages.sort();
            let mut expected = (0..6)
                .map(|index| format!("value-{index:02}"))
                .chain(std::iter::once("staged".to_string()))
                .collect::<Vec<_>>();
            expected.sort();
            assert_eq!(
                all_pages, expected,
                "portal pages must come from one result; first={first_page:?} second={second_page:?}"
            );

            drop(reader_socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_page_wildcard_portal_from_one_result_when_source_has_no_collection_schema() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("portal-schemaless-concurrent-create");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for name in ["portal_catalog_b", "portal_catalog_c", "portal_catalog_d"] {
                cassie
                    .execute_sql(
                        &session,
                        &format!("CREATE TABLE {name} (payload TEXT)"),
                        vec![],
                    )
                    .expect("create table");
            }
            let observed = cassie.clone();
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("query connection");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame("catalog_stmt", "SELECT * FROM information_schema.tables"),
                    support::bind_frame("catalog_portal", "catalog_stmt", &[]),
                    support::describe_portal_frame("catalog_portal"),
                    support::execute_limited_frame("catalog_portal", 1),
                    support::sync_frame(),
                ],
            )
            .await;
            let first_frames = support::read_frames_until_ready(&mut reader).await;
            observed
                .execute_sql(
                    &session,
                    "CREATE TABLE portal_catalog_a (payload TEXT)",
                    vec![],
                )
                .expect("concurrent create table");

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::execute_limited_frame("catalog_portal", 1000),
                    support::sync_frame(),
                ],
            )
            .await;
            let second_frames = support::read_frames_until_ready(&mut reader).await;

            // Assert
            let names = support::row_description_names(&first_frames);
            let table_name = names
                .iter()
                .position(|name| name == "table_name")
                .unwrap_or_else(|| panic!("table_name column: {names:?} {first_frames:?}"));
            let first_page = support::data_rows(&first_frames);
            let second_page = support::data_rows(&second_frames);
            let table_names = first_page
                .iter()
                .chain(second_page.iter())
                .filter_map(|row| row[table_name].clone())
                .filter(|name| name.starts_with("portal_catalog_"))
                .collect::<Vec<_>>();
            assert_eq!(
                table_names,
                vec![
                    "portal_catalog_b".to_string(),
                    "portal_catalog_c".to_string(),
                    "portal_catalog_d".to_string(),
                ],
                "first={first_page:?} second={second_page:?}"
            );

            drop(socket);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    fn declared_id_table(path: &str, table: &str) -> (Cassie, String) {
        let cassie = Cassie::new_with_data_dir(path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            format!("CREATE TABLE {table} (id TEXT, title TEXT)"),
            format!("INSERT INTO {table} (id, title) VALUES ('a', 'x')"),
        ] {
            cassie
                .execute_sql(&session, &sql, vec![])
                .unwrap_or_else(|error| panic!("{sql}: {error}"));
        }
        let selected = cassie
            .execute_sql(&session, &format!("SELECT id FROM {table}"), vec![])
            .expect("select row id");
        let cassie::types::Value::String(row_id) = selected.rows[0][0].clone() else {
            panic!("row id should be text");
        };
        (cassie, row_id)
    }

    fn data_values(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'D')
            .filter_map(|(_, payload)| {
                support::parse_data_row(payload)
                    .into_iter()
                    .next()
                    .flatten()
            })
            .collect()
    }
}

// Formerly tests/pgwire_simple_query_batch.rs.
mod pgwire_simple_query_batch {
    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use std::net::SocketAddr;
    use std::time::Duration;

    type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;
    type WireFrame = (u8, Vec<u8>);

    use super::support_sql as support;
    use support::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn data_dir(label: &str) -> String {
        crate::support_temp_dirs::sweep_stale_once();
        let mut path = std::env::temp_dir();
        path.push(format!("cassie-pgwire-simple-query-batch-{label}"));
        path.push(uuid::Uuid::new_v4().to_string());
        path.to_string_lossy().to_string()
    }

    fn new_cassie(path: &str) -> Cassie {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        Cassie::new_with_data_dir_and_config(path, config).expect("cassie")
    }

    fn startup_frame() -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
        payload.extend_from_slice(b"user\0root\0database\0postgres\0\0");

        let mut frame = Vec::new();
        frame.extend_from_slice(
            &i32::try_from(payload.len() + 4)
                .expect("startup payload size must fit into i32")
                .to_be_bytes(),
        );
        frame.extend_from_slice(&payload);
        frame
    }

    fn simple_query_frame(sql: &str) -> Vec<u8> {
        let mut payload = sql.as_bytes().to_vec();
        payload.push(0);

        let mut frame = vec![b'Q'];
        frame.extend_from_slice(
            &i32::try_from(payload.len() + 4)
                .expect("query payload size must fit into i32")
                .to_be_bytes(),
        );
        frame.extend_from_slice(&payload);
        frame
    }

    fn password_frame(password: &str) -> Vec<u8> {
        let mut payload = password.as_bytes().to_vec();
        payload.push(0);

        let mut frame = vec![b'p'];
        frame.extend_from_slice(
            &i32::try_from(payload.len() + 4)
                .expect("password payload size must fit into i32")
                .to_be_bytes(),
        );
        frame.extend_from_slice(&payload);
        frame
    }

    async fn spawn_server(cassie: &Cassie) -> (SocketAddr, PgwireServer) {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            address.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        (address, server)
    }

    async fn read_wire_frame(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    ) -> WireFrame {
        let mut tag = [0_u8; 1];
        tokio::io::AsyncReadExt::read_exact(reader, &mut tag)
            .await
            .expect("read frame tag");
        let mut length = [0_u8; 4];
        tokio::io::AsyncReadExt::read_exact(reader, &mut length)
            .await
            .expect("read frame length");
        let length = i32::from_be_bytes(length);
        let payload_length = usize::try_from(length - 4).expect("valid frame length");
        let mut payload = vec![0_u8; payload_length];
        if payload_length > 0 {
            tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
                .await
                .expect("read frame payload");
        }
        (tag[0], payload)
    }

    async fn start_session(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
        writer: &mut tokio::net::tcp::WriteHalf<'_>,
    ) {
        tokio::io::AsyncWriteExt::write_all(writer, &startup_frame())
            .await
            .expect("write startup");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush startup");
        let authentication = read_wire_frame(reader).await;
        assert_eq!(authentication.0, b'R');
        assert_eq!(
            i32::from_be_bytes(
                authentication.1[0..4]
                    .try_into()
                    .expect("authentication status"),
            ),
            3
        );
        tokio::io::AsyncWriteExt::write_all(writer, &password_frame("postgres"))
            .await
            .expect("write password");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush password");
        let authentication_ok = read_wire_frame(reader).await;
        assert_eq!(authentication_ok.0, b'R');
        assert_eq!(
            i32::from_be_bytes(
                authentication_ok.1[0..4]
                    .try_into()
                    .expect("authentication status"),
            ),
            0
        );
        let mut saw_backend_key = false;
        loop {
            let frame = read_wire_frame(reader).await;
            if frame.0 == b'Z' {
                assert_eq!(frame.1, vec![b'I']);
                break;
            }
            match frame.0 {
                b'S' => {}
                b'K' => {
                    assert!(!saw_backend_key, "startup emitted duplicate backend key");
                    assert_eq!(frame.1.len(), 8, "backend key payload");
                    saw_backend_key = true;
                }
                tag => panic!("unexpected startup frame: {}", char::from(tag)),
            }
        }
        assert!(saw_backend_key, "startup should emit backend key data");
    }

    async fn send_query(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
        writer: &mut tokio::net::tcp::WriteHalf<'_>,
        sql: &str,
    ) -> Vec<WireFrame> {
        tokio::io::AsyncWriteExt::write_all(writer, &simple_query_frame(sql))
            .await
            .expect("write query");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush query");
        let mut frames = Vec::new();
        loop {
            let frame = read_wire_frame(reader).await;
            let ready = frame.0 == b'Z';
            frames.push(frame);
            if ready {
                return frames;
            }
        }
    }

    fn cstring(payload: &[u8]) -> String {
        let end = payload
            .iter()
            .position(|byte| *byte == 0)
            .expect("cstring terminator");
        String::from_utf8(payload[..end].to_vec()).expect("cstring utf-8")
    }

    fn command(frame: &WireFrame) -> String {
        assert_eq!(frame.0, b'C', "expected command complete frame");
        cstring(&frame.1)
    }

    fn data_row(frame: &WireFrame) -> Vec<Option<String>> {
        assert_eq!(frame.0, b'D', "expected data row frame");
        let mut cursor = 0usize;
        let count = i16::from_be_bytes(frame.1[0..2].try_into().expect("column count"));
        cursor += 2;
        let mut values = Vec::new();
        for _ in 0..count {
            let length = i32::from_be_bytes(
                frame.1[cursor..cursor + 4]
                    .try_into()
                    .expect("value length"),
            );
            cursor += 4;
            if length < 0 {
                values.push(None);
                continue;
            }
            let length = usize::try_from(length).expect("value length fits usize");
            let end = cursor + length;
            values.push(Some(
                String::from_utf8(frame.1[cursor..end].to_vec()).expect("data row utf-8"),
            ));
            cursor = end;
        }
        values
    }

    fn error_code(frame: &WireFrame) -> Option<String> {
        assert_eq!(frame.0, b'E', "expected error response frame");
        let mut cursor = 0usize;
        while cursor < frame.1.len() && frame.1[cursor] != 0 {
            let field = frame.1[cursor];
            cursor += 1;
            let remaining = &frame.1[cursor..];
            let end = remaining
                .iter()
                .position(|byte| *byte == 0)
                .expect("error field terminator");
            if field == b'C' {
                return Some(String::from_utf8(remaining[..end].to_vec()).expect("sqlstate utf-8"));
            }
            cursor += end + 1;
        }
        None
    }

    fn assert_ready(frames: &[WireFrame]) {
        assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(b"I".as_slice())
        );
    }

    #[test]
    fn should_execute_simple_query_statements_in_order_with_one_ready_frame() {
        // Arrange
        use_local_storage();
        let path = data_dir("ordered");

        runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            "CREATE TABLE batch_order (number INT); INSERT INTO batch_order (number) VALUES (1); SELECT number FROM batch_order ORDER BY number",
        )
        .await;
        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'C', b'C', b'T', b'D', b'C', b'Z']
        );
        assert_eq!(command(&frames[0]), "CREATE TABLE");
        assert_eq!(command(&frames[1]), "INSERT 0 1");
        assert_eq!(data_row(&frames[3]), vec![Some("1".to_string())]);
        assert_ready(&frames);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_preserve_semicolon_delimiters_in_quoted_commented_sql() {
        // Arrange
        use_local_storage();
        let path = data_dir("quotes-comments");

        runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            "CREATE TABLE \"batch;quoted\" (note TEXT); -- comment;\nINSERT INTO \"batch;quoted\" (note) VALUES ('text;value'); /* block; comment */ SELECT note FROM \"batch;quoted\"",
        )
        .await;
        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'C', b'C', b'T', b'D', b'C', b'Z']
        );
        assert_eq!(data_row(&frames[3]), vec![Some("text;value".to_string())]);
        assert_ready(&frames);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_ignore_empty_statements_in_a_simple_query_batch() {
        // Arrange
        use_local_storage();
        let path = data_dir("empty");

        runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            ";; CREATE TABLE batch_empty (number INT); ; INSERT INTO batch_empty (number) VALUES (7); ;; SELECT number FROM batch_empty",
        )
        .await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'C', b'C', b'T', b'D', b'C', b'Z']
        );
        assert_eq!(data_row(&frames[3]), vec![Some("7".to_string())]);
        assert_ready(&frames);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_stop_after_the_first_error_in_a_simple_query_batch() {
        // Arrange
        use_local_storage();
        let path = data_dir("stop-on-error");

        runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            "CREATE TABLE batch_stop (number INT); INSERT INTO batch_stop (number) VALUES (1); SELECT number FROM missing_batch_stop; INSERT INTO batch_stop (number) VALUES (2)",
        )
        .await;
        let after_error = send_query(
            &mut reader,
            &mut writer,
            "SELECT number FROM batch_stop ORDER BY number",
        )
        .await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'C', b'C', b'E', b'Z']
        );
        assert_ready(&frames);
        assert_eq!(
            after_error.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'T', b'D', b'C', b'Z']
        );
        assert_eq!(data_row(&after_error[1]), vec![Some("1".to_string())]);
        assert_ready(&after_error);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_transaction_control_statements_in_order() {
        // Arrange
        use_local_storage();
        let path = data_dir("transactions");

        runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            "CREATE TABLE batch_transaction (number INT); BEGIN; INSERT INTO batch_transaction (number) VALUES (9); COMMIT; SELECT number FROM batch_transaction",
        )
        .await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'C', b'C', b'C', b'C', b'T', b'D', b'C', b'Z']
        );
        assert_eq!(command(&frames[1]), "BEGIN");
        assert_eq!(command(&frames[2]), "INSERT 0 1");
        assert_eq!(command(&frames[3]), "COMMIT");
        assert_eq!(data_row(&frames[5]), vec![Some("9".to_string())]);
        assert_ready(&frames);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_copy_when_mixed_with_other_simple_query_statements() {
        // Arrange
        use_local_storage();
        let path = data_dir("copy-mixed");

        runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            "CREATE TABLE batch_copy (number INT); COPY batch_copy FROM STDIN WITH (FORMAT csv); SELECT number FROM batch_copy",
        )
        .await;
        let no_partial_table = send_query(&mut reader, &mut writer, "SELECT number FROM batch_copy").await;

        // Assert
        assert_eq!(frames.iter().map(|frame| frame.0).collect::<Vec<_>>(), vec![b'E', b'Z']);
        assert_eq!(error_code(&frames[0]).as_deref(), Some("0A000"));
        assert_ready(&frames);
        assert_eq!(
            no_partial_table.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'E', b'Z']
        );
        assert_eq!(error_code(&no_partial_table[0]).as_deref(), Some("42P01"));
        assert_ready(&no_partial_table);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
    }
}

mod pgwire_portal_completion {
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use tokio::io::BufReader;

    use super::support_pgwire as support;

    type Frames = Vec<(u8, Vec<u8>)>;
    type PortalPages = (Vec<String>, Vec<String>);

    fn seeded_cassie(path: &str, table: &str, count: usize) -> Cassie {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        let cassie = Cassie::new_with_data_dir_and_config(path, config).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {table} (payload TEXT)"),
                vec![],
            )
            .expect("create table");
        for index in 0..count {
            cassie
                .midge
                .put_document(
                    table,
                    Some(format!("doc-{index:02}")),
                    serde_json::json!({"payload": format!("value-{index:02}")}),
                )
                .expect("seed row");
        }
        cassie
    }

    fn data_values(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'D')
            .filter_map(|(_, payload)| {
                support::parse_data_row(payload)
                    .into_iter()
                    .next()
                    .flatten()
            })
            .collect()
    }

    fn command_tags(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'C')
            .map(|(_, payload)| {
                String::from_utf8_lossy(payload)
                    .trim_end_matches('\0')
                    .to_string()
            })
            .collect()
    }

    fn error_code(frames: &[(u8, Vec<u8>)]) -> Option<String> {
        let (_, payload) = frames.iter().find(|(tag, _)| *tag == b'E')?;
        support::parse_error_fields(payload)
            .into_iter()
            .find_map(|(tag, value)| (tag == 'C').then_some(value))
    }

    async fn round_trip(
        reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
        writer: &mut tokio::net::tcp::WriteHalf<'_>,
        frames: Vec<Vec<u8>>,
    ) -> Frames {
        support::write_frames(writer, frames).await;
        support::read_frames_until_ready(reader).await
    }

    /// Opens a transaction, suspends a streaming portal after two rows, stages
    /// `staged_sql` in the same transaction, and resumes the portal.
    fn resume_after_staged_write(label: &str, staged_sql: &str) -> PortalPages {
        support::use_local_storage();
        let path = support::data_dir(label);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let pages = runtime.block_on(async {
            let cassie = seeded_cassie(&path, "portal_staged_resume", 6);
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            let begin = round_trip(
                &mut reader,
                &mut write_half,
                vec![support::simple_query_frame("BEGIN")],
            )
            .await;
            assert!(begin.iter().all(|(tag, _)| *tag != b'E'));
            let first = round_trip(
                &mut reader,
                &mut write_half,
                vec![
                    support::parse_frame("resume_stmt", "SELECT payload FROM portal_staged_resume"),
                    support::bind_frame("resume_portal", "resume_stmt", &[]),
                    support::execute_limited_frame("resume_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;
            assert!(first.iter().any(|(tag, _)| *tag == b's'), "{first:?}");
            let staged = round_trip(
                &mut reader,
                &mut write_half,
                vec![support::simple_query_frame(staged_sql)],
            )
            .await;
            assert!(
                staged.iter().all(|(tag, _)| *tag != b'E'),
                "{staged_sql} failed"
            );
            let second = round_trip(
                &mut reader,
                &mut write_half,
                vec![
                    support::execute_limited_frame("resume_portal", 100),
                    support::sync_frame(),
                ],
            )
            .await;
            assert!(second.iter().all(|(tag, _)| *tag != b'E'), "{second:?}");
            drop(socket);
            server.stop().await;
            (data_values(&first), data_values(&second))
        });
        let _ = std::fs::remove_dir_all(path);
        pages
    }

    fn original_rows() -> Vec<String> {
        (0..6).map(|index| format!("value-{index:02}")).collect()
    }

    #[test]
    fn should_resume_suspended_portal_from_original_result_after_staged_insert() {
        // Arrange
        let staged_sql = "INSERT INTO portal_staged_resume (payload) VALUES ('staged')";

        // Act
        let (first, second) = resume_after_staged_write("portal-staged-insert", staged_sql);

        // Assert
        assert_eq!(first, original_rows()[..2].to_vec());
        assert_eq!(second, original_rows()[2..].to_vec());
    }

    #[test]
    fn should_resume_suspended_portal_from_original_result_after_staged_delete() {
        // Arrange
        let staged_sql = "DELETE FROM portal_staged_resume WHERE payload = 'value-00'";

        // Act
        let (first, second) = resume_after_staged_write("portal-staged-delete", staged_sql);

        // Assert
        assert_eq!(first, original_rows()[..2].to_vec());
        assert_eq!(second, original_rows()[2..].to_vec());
    }

    #[test]
    fn should_return_select_zero_when_completed_select_portal_executes_again() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("portal-completed-select");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = seeded_cassie(&path, "portal_completed_select", 3);
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            let first = round_trip(
                &mut reader,
                &mut write_half,
                vec![
                    support::parse_frame(
                        "completed_select",
                        "SELECT payload FROM portal_completed_select",
                    ),
                    support::bind_frame("completed_select_portal", "completed_select", &[]),
                    support::execute_frame("completed_select_portal"),
                    support::sync_frame(),
                ],
            )
            .await;
            assert_eq!(data_values(&first).len(), 3);

            // Act
            let again = round_trip(
                &mut reader,
                &mut write_half,
                vec![
                    support::execute_frame("completed_select_portal"),
                    support::sync_frame(),
                ],
            )
            .await;

            // Assert
            assert!(data_values(&again).is_empty(), "{again:?}");
            assert_eq!(command_tags(&again), vec!["SELECT 0".to_string()]);

            drop(socket);
            server.stop().await;
        });
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_not_rerun_insert_when_completed_insert_portal_executes_again() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("portal-completed-insert");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = seeded_cassie(&path, "portal_completed_insert", 0);
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            let first = round_trip(
                &mut reader,
                &mut write_half,
                vec![
                    support::parse_frame(
                        "completed_insert",
                        "INSERT INTO portal_completed_insert (payload) VALUES ('once')",
                    ),
                    support::bind_frame("completed_insert_portal", "completed_insert", &[]),
                    support::execute_frame("completed_insert_portal"),
                    support::sync_frame(),
                ],
            )
            .await;
            assert_eq!(command_tags(&first), vec!["INSERT 0 1".to_string()]);

            // Act
            let again = round_trip(
                &mut reader,
                &mut write_half,
                vec![
                    support::execute_frame("completed_insert_portal"),
                    support::sync_frame(),
                ],
            )
            .await;
            let selected = round_trip(
                &mut reader,
                &mut write_half,
                vec![support::simple_query_frame(
                    "SELECT payload FROM portal_completed_insert",
                )],
            )
            .await;

            // Assert
            assert_eq!(command_tags(&again), vec!["INSERT 0 0".to_string()]);
            assert_eq!(data_values(&selected), vec!["once".to_string()]);

            drop(socket);
            server.stop().await;
        });
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_drop_suspended_portal_when_extended_commit_ends_transaction() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("portal-extended-commit");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = seeded_cassie(&path, "portal_extended_commit", 4);
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            let _ = round_trip(
                &mut reader,
                &mut write_half,
                vec![support::simple_query_frame("BEGIN")],
            )
            .await;
            let first = round_trip(
                &mut reader,
                &mut write_half,
                vec![
                    support::parse_frame(
                        "commit_select",
                        "SELECT payload FROM portal_extended_commit",
                    ),
                    support::bind_frame("commit_portal", "commit_select", &[]),
                    support::execute_limited_frame("commit_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;
            assert!(first.iter().any(|(tag, _)| *tag == b's'), "{first:?}");
            let committed = round_trip(
                &mut reader,
                &mut write_half,
                vec![
                    support::parse_frame("commit_stmt", "COMMIT"),
                    support::bind_frame("commit_stmt_portal", "commit_stmt", &[]),
                    support::execute_frame("commit_stmt_portal"),
                    support::sync_frame(),
                ],
            )
            .await;
            assert!(
                committed.iter().all(|(tag, _)| *tag != b'E'),
                "{committed:?}"
            );

            // Act
            let resumed = round_trip(
                &mut reader,
                &mut write_half,
                vec![
                    support::execute_limited_frame("commit_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;

            // Assert
            assert!(data_values(&resumed).is_empty(), "{resumed:?}");
            assert_eq!(error_code(&resumed).as_deref(), Some("26000"));

            drop(socket);
            server.stop().await;
        });
        let _ = std::fs::remove_dir_all(path);
    }
}
