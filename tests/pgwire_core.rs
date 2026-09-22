// Consolidated integration suite: pgwire_core.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/pgwire.rs.
mod pgwire {
    use cassie::pgwire::protocol::{
        decode, encode, ClientMessage, RowDescriptionField, ServerMessage,
    };

    #[test]
    fn should_decode_basic_protocol_messages() {
        // Arrange
        let startup = decode("STARTUP user=alice database=testdb");
        let query = decode("QUERY select 1");

        // Act
        let encoded = encode(&ServerMessage::RowDescription(vec![
            RowDescriptionField {
                name: "id".to_string(),
                data_type: "text".to_string(),
                type_oid: 25,
                typlen: -1,
                atttypmod: -1,
                format_code: 0,
                nullable: true,
            },
            RowDescriptionField {
                name: "score".to_string(),
                data_type: "float".to_string(),
                type_oid: 701,
                typlen: 8,
                atttypmod: -1,
                format_code: 0,
                nullable: true,
            },
        ]));
        let raw = String::from_utf8_lossy(&encoded);
        let payload = raw
            .strip_prefix("ROWDESC ")
            .and_then(|value| value.strip_suffix('\n'))
            .unwrap_or_default();
        let decoded: Vec<RowDescriptionField> =
            serde_json::from_str(payload).expect("row description payload should be valid json");

        // Assert
        let ClientMessage::Startup { user, database } = startup else {
            panic!("expected startup message");
        };
        assert_eq!(user, "alice");
        assert_eq!(database, Some("testdb".to_string()));

        let ClientMessage::Query(sql) = query else {
            panic!("expected query message");
        };
        assert_eq!(sql, "select 1");
        assert_eq!(decoded[0].name, "id");
        assert_eq!(decoded[1].type_oid, 701);
        assert_eq!(encoded.last(), Some(&b'\n'));
    }

    #[test]
    fn should_decode_extended_protocol_lifecycle_messages() {
        // Arrange
        let parse = decode("PARSE q1|SELECT * FROM items");
        let bind = decode("BIND q1 $1|$2");
        let describe = decode("DESCRIBE q1");
        let execute = decode("EXECUTE q1 3");
        let close = decode("CLOSE q1");

        // Act
        let ClientMessage::Parse { name, query } = parse else {
            panic!("expected parse message");
        };

        // Assert
        assert_eq!(name, "q1");
        assert_eq!(query, "SELECT * FROM items");

        let ClientMessage::Bind { name, params } = bind else {
            panic!("expected bind message");
        };
        assert_eq!(name, "q1");
        assert_eq!(params, vec!["$1", "$2"]);

        let ClientMessage::Describe(name) = describe else {
            panic!("expected describe message");
        };
        assert_eq!(name, "q1");

        let ClientMessage::Execute { name, limit } = execute else {
            panic!("expected execute message");
        };
        assert_eq!(name, "q1");
        assert_eq!(limit, Some(3));

        let ClientMessage::Close(name) = close else {
            panic!("expected close message");
        };
        assert_eq!(name, "q1");
    }
}

// Formerly tests/pgwire_binary_codecs.rs.
mod pgwire_binary_codecs {
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

    #[test]
    fn should_describe_case_result_types_over_pgwire() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("case-result-types");
        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");

            // Act
            let (frames, server) = start_extended_query(
                cassie,
                support::parse_frame("case_stmt", "SELECT CASE WHEN true THEN CAST(2 AS INT) ELSE CAST(3 AS BIGINT) END AS n, CASE WHEN false THEN 'x' ELSE 'y' END AS t, CASE WHEN true THEN CAST(2 AS INT) ELSE 3.5 END AS widened"),
                support::bind_frame_with_formats("case_portal", "case_stmt", &[], &[], &[1]),
                support::execute_frame("case_portal"),
            ).await;

            // Assert
            let description = frames.iter().find(|frame| frame.0 == b'T').expect("row description");
            let fields = support::parse_row_description(&description.1);
            assert_eq!(fields.iter().map(|field| field.type_oid).collect::<Vec<_>>(), vec![20, 25, 701]);
            let row = frames.iter().find(|frame| frame.0 == b'D').expect("data row");
            assert_eq!(read_binary_row(&row.1), vec![Some(2_i64.to_be_bytes().to_vec()), Some(b"y".to_vec()), Some(2_f64.to_be_bytes().to_vec())]);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_encode_binary_result_codecs_with_exact_bytes() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("binary-result-codecs");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        seed_result_codecs(&cassie);

        // Act
        let (frames, server) = start_extended_query(
            cassie,
            support::parse_frame(
                "binary_result_stmt",
                "SELECT small, regular, wide, ratio, flag, blob, item_uuid, created_on, created_at, created_at_ts, title, code, alias, payload, nullable FROM binary_codec_docs",
            ),
            support::bind_frame_with_formats(
                "binary_result_portal",
                "binary_result_stmt",
                &[],
                &[],
                &[1],
            ),
            support::execute_frame("binary_result_portal"),
        )
        .await;

        // Assert
        let row_description = frames.iter().find(|frame| frame.0 == b'T').expect("row description");
        let fields = support::parse_row_description(&row_description.1);
        assert_eq!(fields.len(), 15);
        assert!(fields.iter().all(|field| field.format_code == 1));
        assert_eq!(fields[0].type_oid, 21);
        assert_eq!(fields[5].type_oid, 17);
        assert_eq!(fields[6].type_oid, 2950);
        assert_eq!(fields[7].type_oid, 1082);
        assert_eq!(fields[8].type_oid, 1083);
        assert_eq!(fields[9].type_oid, 1114);
        let row = frames.iter().find(|frame| frame.0 == b'D').expect("data row");
        assert_eq!(
            read_binary_row(&row.1),
            vec![
                Some((-12_i16).to_be_bytes().to_vec()),
                Some(123_456_789_i32.to_be_bytes().to_vec()),
                Some((-9_223_372_036_854_775_000_i64).to_be_bytes().to_vec()),
                Some(3.5_f64.to_be_bytes().to_vec()),
                Some(vec![1]),
                Some(vec![1, 2, 10]),
                Some(vec![0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0x00, 0x00]),
                Some(1_i32.to_be_bytes().to_vec()),
                Some(1_000_002_i64.to_be_bytes().to_vec()),
                Some(86_401_000_003_i64.to_be_bytes().to_vec()),
                Some(b"text".to_vec()),
                Some(b"ABCD".to_vec()),
                Some(b"varchar".to_vec()),
                Some(br#"{"a":1}"#.to_vec()),
                None,
            ]
        );
        assert_eq!(frames.last().map(|frame| frame.1.as_slice()), Some(b"I".as_slice()));

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_decode_binary_temporal_uuid_parameters() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("binary-parameter-codecs");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        seed_parameter_codecs(&cassie);
        let uuid = [
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55,
            0x44, 0x00, 0x00,
        ];

        // Act
        let (frames, server) = start_extended_query(
            cassie,
            support::parse_frame_with_types(
                "binary_parameter_stmt",
                "INSERT INTO binary_parameter_docs (item_uuid, created_on, created_at, created_at_ts, blob) VALUES ($1, $2, $3, $4, $5) RETURNING item_uuid, created_on, created_at, created_at_ts, blob",
                &[2950, 1082, 1083, 1114, 17],
            ),
            support::bind_frame_with_formats(
                "binary_parameter_portal",
                "binary_parameter_stmt",
                &[1],
                &[
                    Some(&uuid),
                    Some(&1_i32.to_be_bytes()),
                    Some(&1_000_002_i64.to_be_bytes()),
                    Some(&86_401_000_003_i64.to_be_bytes()),
                    Some(&[0xde, 0xad]),
                ],
                &[0],
            ),
            support::execute_frame("binary_parameter_portal"),
        )
        .await;

        // Assert
        let row = frames.iter().find(|frame| frame.0 == b'D').expect("data row");
        assert_eq!(
            support::parse_data_row(&row.1),
            vec![
                Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
                Some("2000-01-02".to_string()),
                Some("00:00:01.000002".to_string()),
                Some("2000-01-02T00:00:01.000003Z".to_string()),
                Some("\\xdead".to_string()),
            ]
        );
        assert_eq!(frames.last().map(|frame| frame.1.as_slice()), Some(b"I".as_slice()));

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_encode_binary_vector_array_codecs() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("binary-unsupported-codecs");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            seed_unsupported_codecs(&cassie);

            // Act
            let (frames, server) = start_extended_query(
                cassie,
                support::parse_frame(
                    "unsupported_binary_stmt",
                    "SELECT embedding, values FROM binary_unsupported_docs",
                ),
                support::bind_frame_with_formats(
                    "unsupported_binary_portal",
                    "unsupported_binary_stmt",
                    &[],
                    &[],
                    &[1],
                ),
                support::execute_frame("unsupported_binary_portal"),
            )
            .await;

            // Assert
            let row = frames
                .iter()
                .find(|frame| frame.0 == b'D')
                .expect("data row");
            assert_eq!(
                read_binary_row(&row.1),
                vec![
                    Some(
                        [
                            2_i16.to_be_bytes().as_slice(),
                            0_i16.to_be_bytes().as_slice(),
                            1.0_f32.to_be_bytes().as_slice(),
                            2.0_f32.to_be_bytes().as_slice(),
                        ]
                        .concat()
                    ),
                    Some(
                        [
                            1_i32.to_be_bytes().as_slice(),
                            0_i32.to_be_bytes().as_slice(),
                            23_i32.to_be_bytes().as_slice(),
                            2_i32.to_be_bytes().as_slice(),
                            1_i32.to_be_bytes().as_slice(),
                            4_i32.to_be_bytes().as_slice(),
                            1_i32.to_be_bytes().as_slice(),
                            4_i32.to_be_bytes().as_slice(),
                            2_i32.to_be_bytes().as_slice(),
                        ]
                        .concat()
                    ),
                ]
            );
            assert_eq!(
                frames.last().map(|frame| frame.1.as_slice()),
                Some(b"I".as_slice())
            );

            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_decode_binary_vector_array_parameters() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("binary-vector-array-parameters");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE binary_parameter_complex (embedding VECTOR(2), values INT[])",
                Vec::new(),
            )
            .expect("create complex parameter table");
        let vector = [
            2_i16.to_be_bytes().as_slice(),
            0_i16.to_be_bytes().as_slice(),
            1.5_f32.to_be_bytes().as_slice(),
            (-2.0_f32).to_be_bytes().as_slice(),
        ]
        .concat();
        let array = [
            1_i32.to_be_bytes().as_slice(),
            0_i32.to_be_bytes().as_slice(),
            23_i32.to_be_bytes().as_slice(),
            3_i32.to_be_bytes().as_slice(),
            1_i32.to_be_bytes().as_slice(),
            4_i32.to_be_bytes().as_slice(),
            7_i32.to_be_bytes().as_slice(),
            4_i32.to_be_bytes().as_slice(),
            8_i32.to_be_bytes().as_slice(),
            4_i32.to_be_bytes().as_slice(),
            9_i32.to_be_bytes().as_slice(),
        ]
        .concat();

        // Act
        let (frames, server) = start_extended_query(
            cassie,
            support::parse_frame_with_types(
                "binary_complex_parameter_stmt",
                "INSERT INTO binary_parameter_complex (embedding, values) VALUES ($1, $2) RETURNING embedding, values",
                &[33_002, 34_023],
            ),
            support::bind_frame_with_formats(
                "binary_complex_parameter_portal",
                "binary_complex_parameter_stmt",
                &[1],
                &[Some(&vector), Some(&array)],
                &[0],
            ),
            support::execute_frame("binary_complex_parameter_portal"),
        )
        .await;

        // Assert
        let row = frames.iter().find(|frame| frame.0 == b'D').expect("data row");
        assert_eq!(
            support::parse_data_row(&row.1),
            vec![Some("[1.5,-2.0]".to_string()), Some("[7,8,9]".to_string())]
        );

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_binary_array_lengths_that_exceed_the_payload() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("binary-array-length");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let array = [
                1_i32.to_be_bytes().as_slice(),
                0_i32.to_be_bytes().as_slice(),
                23_i32.to_be_bytes().as_slice(),
                i32::MAX.to_be_bytes().as_slice(),
                1_i32.to_be_bytes().as_slice(),
            ]
            .concat();

            // Act
            let (frames, server) = start_extended_query(
                cassie,
                support::parse_frame_with_types(
                    "invalid_array_length_stmt",
                    "SELECT $1",
                    &[34_023],
                ),
                support::bind_frame_with_formats(
                    "invalid_array_length_portal",
                    "invalid_array_length_stmt",
                    &[1],
                    &[Some(&array)],
                    &[],
                ),
                support::execute_frame("invalid_array_length_portal"),
            )
            .await;

            // Assert
            assert!(frames.iter().any(|frame| frame.0 == b'E'));
            assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));

            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_apply_mixed_result_formats_with_null_values() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("binary-mixed-formats");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            seed_result_codecs(&cassie);

            // Act
            let (frames, server) = start_extended_query(
                cassie,
                support::parse_frame(
                    "mixed_binary_stmt",
                    "SELECT nullable, regular FROM binary_codec_docs",
                ),
                support::bind_frame_with_formats(
                    "mixed_binary_portal",
                    "mixed_binary_stmt",
                    &[],
                    &[],
                    &[0, 1],
                ),
                support::execute_frame("mixed_binary_portal"),
            )
            .await;

            // Assert
            let row_description = frames
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("row description");
            let fields = support::parse_row_description(&row_description.1);
            assert_eq!(
                fields
                    .iter()
                    .map(|field| field.format_code)
                    .collect::<Vec<_>>(),
                vec![0, 1]
            );
            let row = frames
                .iter()
                .find(|frame| frame.0 == b'D')
                .expect("data row");
            assert_eq!(
                read_binary_row(&row.1),
                vec![None, Some(123_456_789_i32.to_be_bytes().to_vec())]
            );
            assert_eq!(
                frames.last().map(|frame| frame.1.as_slice()),
                Some(b"I".as_slice())
            );

            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_encode_integer_sum_as_exact_binary_int8() {
        // Arrange
        const EXACT_SUM: i64 = 9_007_199_254_740_993;
        support::use_local_storage();
        let path = support::data_dir("binary-integer-sum");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE binary_sum_docs (wide BIGINT)",
                    Vec::new(),
                )
                .expect("create binary sum table");
            for value in [EXACT_SUM, 0] {
                cassie
                    .execute_sql(
                        &session,
                        "INSERT INTO binary_sum_docs (wide) VALUES ($1)",
                        vec![Value::Int64(value)],
                    )
                    .expect("insert binary sum row");
            }

            // Act
            let (frames, server) = start_extended_query(
                cassie,
                support::parse_frame("binary_sum_stmt", "SELECT SUM(wide) FROM binary_sum_docs"),
                support::bind_frame_with_formats(
                    "binary_sum_portal",
                    "binary_sum_stmt",
                    &[],
                    &[],
                    &[1],
                ),
                support::execute_frame("binary_sum_portal"),
            )
            .await;

            // Assert
            let row_description = frames
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("row description");
            let fields = support::parse_row_description(&row_description.1);
            assert_eq!(fields[0].type_oid, 20, "SUM(bigint) must be declared int8");
            let row = frames
                .iter()
                .find(|frame| frame.0 == b'D')
                .expect("data row");
            assert_eq!(
                read_binary_row(&row.1),
                vec![Some(EXACT_SUM.to_be_bytes().to_vec())]
            );

            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/pgwire_cancellation.rs.
mod pgwire_cancellation {
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

    use super::support_pgwire as support;

    #[test]
    fn should_cancel_only_the_backend_given_matching_process_id_and_secret() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("cancel-active-query");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        config.limits.cte_recursion_depth = 1_000_000;
        config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
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
        let mut query_socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("query connection");
        let (query_read, mut query_write) = query_socket.split();
        let mut query_reader = BufReader::new(query_read);
        let (process_id, secret_key) =
            support::complete_startup_with_backend_key(&mut query_reader, &mut query_write).await;
        query_write
            .write_all(&support::simple_query_frame(
                "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 1000000) SELECT MAX(n) FROM seq",
            ))
            .await
            .expect("write long query");
        query_write.flush().await.expect("flush long query");
        tokio::time::sleep(Duration::from_millis(25)).await;

        // Act
        let mut cancel_socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&support::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write cancel request");
        cancel_socket.shutdown().await.expect("close cancel request");
        let frames = tokio::time::timeout(
            Duration::from_secs(5),
            support::read_frames_until_ready(&mut query_reader),
        )
        .await
        .expect("cancelled query should finish promptly");

        // Assert
        let error = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("query cancellation error");
        let fields = support::parse_error_fields(&error.1);
        assert!(fields
            .iter()
            .any(|(tag, value)| *tag == 'C' && value == "57014"));

        drop(query_socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_ignore_a_cancel_request_given_a_stale_backend_secret() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("cancel-idle-query");
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
                Arc::new(cassie),
                config,
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut query_socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("query connection");
            let (query_read, mut query_write) = query_socket.split();
            let mut query_reader = BufReader::new(query_read);
            let (process_id, secret_key) =
                support::complete_startup_with_backend_key(&mut query_reader, &mut query_write)
                    .await;

            // Act
            let mut cancel_socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("cancel connection");
            cancel_socket
                .write_all(&support::cancel_request_frame(
                    process_id,
                    secret_key.wrapping_add(1),
                ))
                .await
                .expect("write idle cancel request");
            cancel_socket
                .shutdown()
                .await
                .expect("close cancel request");
            let mut cancel_response = Vec::new();
            cancel_socket
                .read_to_end(&mut cancel_response)
                .await
                .expect("wait for idle cancel request processing");
            query_write
                .write_all(&support::simple_query_frame("SELECT 1"))
                .await
                .expect("write query after idle cancellation");
            query_write.flush().await.expect("flush query");
            let frames = support::read_frames_until_ready(&mut query_reader).await;

            // Assert
            assert!(frames.iter().all(|(tag, _)| *tag != b'E'));
            assert!(frames.iter().any(|(tag, _)| *tag == b'D'));

            drop(query_socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/pgwire_copy_recovery.rs.
mod pgwire_copy_recovery {
    use cassie::app::Cassie;
    use tokio::io::AsyncWriteExt;

    use super::support_pgwire as support;

    fn configured_cassie(label: &str) -> (Cassie, String) {
        support::use_local_storage();
        let path = support::data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE pgwire_copy_recovery_rows (id INT, title TEXT)",
                vec![],
            )
            .expect("create copy table");
        (cassie, path)
    }

    #[test]
    fn should_resume_simple_query_processing_after_copy_error() {
        // Arrange
        let (cassie, path) = configured_cassie("copy-simple-recovery");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            write_half
                .write_all(&support::simple_query_frame(
                    "COPY pgwire_copy_recovery_rows (id, title) FROM STDIN WITH (FORMAT csv)",
                ))
                .await
                .expect("request copy");
            write_half.flush().await.expect("flush copy request");
            assert_eq!(support::read_wire_frame(&mut reader).await.0, b'G');

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::copy_data_frame(b"not-an-integer,broken\n"),
                    support::copy_done_frame(),
                ],
            )
            .await;
            let failed = support::read_frames_until_ready(&mut reader).await;
            write_half
                .write_all(&support::simple_query_frame("SELECT 1"))
                .await
                .expect("recovery query");
            write_half.flush().await.expect("flush recovery query");
            let recovered = support::read_frames_until_ready(&mut reader).await;

            // Assert
            assert!(failed.iter().any(|frame| frame.0 == b'E'));
            assert!(recovered.iter().any(|frame| frame.0 == b'D'));
            assert!(recovered.iter().any(|frame| frame.0 == b'C'));
            server.stop().await;
        });
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_invalidate_extended_portals_after_sync_given_a_copy_failure() {
        // Arrange
        let (cassie, path) = configured_cassie("copy-extended-recovery");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame("kept_statement", "SELECT 1"),
                    support::bind_frame("stale_portal", "kept_statement", &[]),
                    support::sync_frame(),
                ],
            )
            .await;
            let prepared = support::read_frames_until_ready(&mut reader).await;
            assert!(prepared.iter().any(|frame| frame.0 == b'2'));

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame(
                        "copy_statement",
                        "COPY pgwire_copy_recovery_rows FROM STDIN WITH (FORMAT csv)",
                    ),
                    support::sync_frame(),
                ],
            )
            .await;
            let failed = support::read_frames_until_ready(&mut reader).await;
            support::write_frames(
                &mut write_half,
                vec![
                    support::execute_frame("stale_portal"),
                    support::sync_frame(),
                ],
            )
            .await;
            let after_sync = support::read_frames_until_ready(&mut reader).await;

            // Assert
            assert!(failed.iter().any(|frame| frame.0 == b'E'));
            assert!(after_sync.iter().any(|frame| frame.0 == b'E'));
            server.stop().await;
        });
        let _ = std::fs::remove_dir_all(path);
    }
}

// COPY sub-protocol state and framing over the simple-query protocol.
mod pgwire_copy_protocol {
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::types::Value;
    use tokio::io::{AsyncRead, AsyncWrite};
    use tokio::net::tcp::{ReadHalf, WriteHalf};

    use super::support_pgwire as support;

    const ANSWER_LIMIT: Duration = Duration::from_secs(10);
    const COPY_ROWS_SQL: &str =
        "COPY pgwire_copy_protocol_rows (id, title) FROM STDIN WITH (FORMAT csv)";

    type Reader<'a> = tokio::io::BufReader<ReadHalf<'a>>;

    fn configured_cassie(label: &str) -> (Cassie, String) {
        support::use_local_storage();
        let path = support::data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE pgwire_copy_protocol_rows (id INT, title TEXT)",
                vec![],
            )
            .expect("create copy table");
        (cassie, path)
    }

    fn row_count(cassie: &Cassie) -> Value {
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT count(*) FROM pgwire_copy_protocol_rows",
                vec![],
            )
            .expect("count copied rows");
        result.rows[0][0].clone()
    }

    async fn query(
        reader: &mut (impl AsyncRead + Unpin),
        writer: &mut (impl AsyncWrite + Unpin),
        sql: &str,
    ) -> Vec<(u8, Vec<u8>)> {
        support::write_frames(writer, vec![support::simple_query_frame(sql)]).await;
        support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await
    }

    async fn start_copy(
        reader: &mut (impl AsyncRead + Unpin),
        writer: &mut (impl AsyncWrite + Unpin),
        sql: &str,
    ) {
        support::write_frames(writer, vec![support::simple_query_frame(sql)]).await;
        assert_eq!(support::read_wire_frame(reader).await.0, b'G');
    }

    fn ready_status(frames: &[(u8, Vec<u8>)]) -> u8 {
        let (tag, payload) = frames.last().expect("ReadyForQuery frame");
        assert_eq!(*tag, b'Z');
        payload[0]
    }

    fn select_one_rows() -> Vec<Vec<Option<String>>> {
        vec![vec![Some("1".to_string())]]
    }

    /// Serves `cassie` over pgwire and runs `scenario` on one connection. The
    /// caller removes the data directory after any post-connection assertions.
    fn run_as<F>(cassie: Cassie, user: (&str, &str), scenario: F)
    where
        F: for<'a, 'b> AsyncFnOnce(&'b mut Reader<'a>, &'b mut WriteHalf<'a>),
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect");
            let (read_half, mut writer) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            support::complete_startup_as(&mut reader, &mut writer, user.0, "postgres", user.1)
                .await;
            scenario(&mut reader, &mut writer).await;
            server.stop().await;
        });
    }

    fn run<F>(cassie: Cassie, scenario: F)
    where
        F: for<'a, 'b> AsyncFnOnce(&'b mut Reader<'a>, &'b mut WriteHalf<'a>),
    {
        run_as(cassie, ("root", "postgres"), scenario);
    }

    #[test]
    fn should_frame_copy_out_response_as_one_text_column_for_database_backup() {
        // Arrange
        let (cassie, path) = configured_cassie("copy-out-framing");

        run(cassie, async |reader, writer| {
            // Act
            let backup = query(reader, writer, "BACKUP DATABASE postgres TO STDOUT").await;

            // Assert
            let (tag, body) = &backup[0];
            assert_eq!(*tag, b'H');
            let columns = usize::try_from(i16::from_be_bytes([body[1], body[2]])).expect("count");
            assert_eq!(body[0], 0);
            assert_eq!(columns, 1);
            assert_eq!(body.len(), 3 + 2 * columns);
            assert!(backup.iter().any(|frame| frame.0 == b'c'));
            assert_eq!(ready_status(&backup), b'I');
        });
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_abort_open_transaction_given_copy_bind_error() {
        // Arrange
        let (cassie, path) = configured_cassie("copy-bind-error-transaction");
        let observer = cassie.clone();

        run(cassie, async |reader, writer| {
            let begin = query(reader, writer, "BEGIN").await;
            assert_eq!(ready_status(&begin), b'T');

            // Act
            let copy = query(
                reader,
                writer,
                "COPY pgwire_copy_protocol_missing FROM STDIN WITH (FORMAT csv)",
            )
            .await;
            let insert = query(
                reader,
                writer,
                "INSERT INTO pgwire_copy_protocol_rows (id, title) VALUES (1, 'kept')",
            )
            .await;
            let _ = query(reader, writer, "COMMIT").await;

            // Assert
            assert!(copy.iter().all(|frame| frame.0 != b'G'));
            assert!(support::error_code(&copy).is_some());
            assert_eq!(ready_status(&copy), b'E');
            assert!(support::error_code(&insert).is_some());
            assert_eq!(ready_status(&insert), b'E');
        });
        assert_eq!(row_count(&observer), Value::Int64(0));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_abort_open_transaction_given_database_copy_privilege_denial() {
        // Arrange
        let (cassie, path) = configured_cassie("database-copy-denial-transaction");
        let admin = cassie
            .authenticate_role("root", Some("postgres"), None)
            .expect("admin");
        for sql in [
            "CREATE ROLE copy_reader LOGIN PASSWORD 'reader-secret'",
            "CREATE DATABASE copy_denied",
            "GRANT CONNECT ON DATABASE postgres TO copy_reader",
        ] {
            cassie
                .execute_sql(&admin, sql, vec![])
                .expect("role fixture");
        }

        run_as(
            cassie,
            ("copy_reader", "reader-secret"),
            async |reader, writer| {
                let begin = query(reader, writer, "BEGIN").await;
                assert_eq!(ready_status(&begin), b'T');

                // Act
                let backup = query(reader, writer, "BACKUP DATABASE copy_denied TO STDOUT").await;

                // Assert
                assert_eq!(support::error_code(&backup).as_deref(), Some("42501"));
                assert_eq!(ready_status(&backup), b'E');
            },
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_answer_next_query_after_copy_payload_exceeds_resource_limit() {
        // Arrange
        let (cassie, path) = configured_cassie("copy-resource-limit-drain");
        let chunk = vec![b'x'; 64 * 1024];

        run(cassie, async |reader, writer| {
            start_copy(reader, writer, COPY_ROWS_SQL).await;
            let mut frames = (0..260)
                .map(|_| support::copy_data_frame(&chunk))
                .collect::<Vec<_>>();
            frames.push(support::copy_done_frame());
            frames.push(support::simple_query_frame("SELECT 1"));

            // Act
            support::write_frames(writer, frames).await;
            let failed = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;
            let recovered = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;

            // Assert
            assert_eq!(support::error_code(&failed).as_deref(), Some("54000"));
            assert_eq!(ready_status(&failed), b'I');
            assert_eq!(support::error_code(&recovered), None);
            assert_eq!(support::data_rows(&recovered), select_one_rows());
            assert_eq!(ready_status(&recovered), b'I');
        });
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_ignore_synchronization_messages_during_copy_in() {
        // Arrange
        let (cassie, path) = configured_cassie("copy-flush-sync");
        let observer = cassie.clone();

        run(cassie, async |reader, writer| {
            start_copy(reader, writer, COPY_ROWS_SQL).await;
            let frames = vec![
                support::copy_data_frame(b"1,alpha\n"),
                support::flush_frame(),
                support::copy_data_frame(b"2,beta\n"),
                support::sync_frame(),
                support::copy_done_frame(),
            ];

            // Act
            support::write_frames(writer, frames).await;
            let copied = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;

            // Assert
            assert_eq!(support::error_code(&copied), None);
            let complete = copied.iter().find(|frame| frame.0 == b'C');
            assert_eq!(
                complete.map(|frame| frame.1.as_slice()),
                Some(&b"COPY 2\0"[..])
            );
            assert_eq!(copied.iter().filter(|frame| frame.0 == b'Z').count(), 1);
            assert_eq!(ready_status(&copied), b'I');
        });
        assert_eq!(row_count(&observer), Value::Int64(2));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_answer_next_query_after_copy_fail_with_trailing_copy_messages() {
        // Arrange
        let (cassie, path) = configured_cassie("copy-fail-trailing");
        let observer = cassie.clone();

        run(cassie, async |reader, writer| {
            start_copy(reader, writer, COPY_ROWS_SQL).await;
            let frames = vec![
                support::copy_data_frame(b"1,alpha\n"),
                support::copy_fail_frame("client aborted"),
                support::copy_data_frame(b"2,late\n"),
                support::copy_done_frame(),
                support::simple_query_frame("SELECT 1"),
            ];

            // Act
            support::write_frames(writer, frames).await;
            let failed = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;
            let recovered = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;

            // Assert
            assert_eq!(support::error_code(&failed).as_deref(), Some("57014"));
            assert_eq!(support::error_code(&recovered), None);
            assert_eq!(support::data_rows(&recovered), select_one_rows());
        });
        assert_eq!(row_count(&observer), Value::Int64(0));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_answer_next_query_after_rejected_restore_image() {
        // Arrange
        let (cassie, path) = configured_cassie("restore-rejected-image");

        run(cassie, async |reader, writer| {
            start_copy(
                reader,
                writer,
                "RESTORE DATABASE restore_rejected FROM STDIN",
            )
            .await;
            let frames = vec![
                support::copy_data_frame(b"NOTCASSIE-bad-image-bytes"),
                support::copy_data_frame(b"more-bad-image-bytes"),
                support::copy_done_frame(),
                support::simple_query_frame("SELECT 1"),
            ];

            // Act
            support::write_frames(writer, frames).await;
            let failed = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;
            let recovered = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;

            // Assert
            assert!(support::error_code(&failed).is_some());
            assert_eq!(ready_status(&failed), b'I');
            assert_eq!(support::error_code(&recovered), None);
            assert_eq!(support::data_rows(&recovered), select_one_rows());
            assert_eq!(ready_status(&recovered), b'I');
        });
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/pgwire_database_images.rs.
mod pgwire_database_images {
    use super::support_pgwire as support;

    use cassie::app::Cassie;
    use cassie::catalog::{
        canonical_relation_name, CollectionMeta, FieldConstraint, IndexKind, IndexMeta,
    };
    use cassie::types::{DataType, FieldSchema, Schema};
    use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
    use uuid::Uuid;

    fn data_dir(label: &str) -> String {
        crate::support_temp_dirs::sweep_stale_once();
        let mut path = std::env::temp_dir();
        path.push(format!("cassie-pgwire-image-{label}-{}", Uuid::new_v4()));
        path.to_string_lossy().into_owned()
    }

    fn query_frame(sql: &str) -> Vec<u8> {
        let mut payload = sql.as_bytes().to_vec();
        payload.push(0);
        let mut frame = vec![b'Q'];
        frame.extend_from_slice(
            &i32::try_from(payload.len() + 4)
                .expect("query frame length")
                .to_be_bytes(),
        );
        frame.extend(payload);
        frame
    }

    fn copy_data_frame(payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![b'd'];
        frame.extend_from_slice(
            &i32::try_from(payload.len() + 4)
                .expect("copy data length")
                .to_be_bytes(),
        );
        frame.extend_from_slice(payload);
        frame
    }

    fn copy_done_frame() -> Vec<u8> {
        vec![b'c', 0, 0, 0, 4]
    }

    fn seed_database_image_fixture(cassie: &Cassie) {
        let collection = canonical_relation_name("analytics", "public", "analytics");
        cassie
            .midge
            .create_database("analytics", None)
            .expect("database");
        cassie
            .midge
            .create_collection_with_meta(
                &collection,
                &Schema {
                    fields: vec![FieldSchema {
                        name: "value".to_string(),
                        data_type: DataType::Text,
                        nullable: false,
                    }],
                },
                &CollectionMeta::new(
                    &collection,
                    Some("analytics text must remain analytics".to_string()),
                ),
            )
            .expect("collection");
        cassie
            .midge
            .save_constraints(
                &collection,
                &[FieldConstraint {
                    default_value: Some(serde_json::json!("analytics")),
                    ..FieldConstraint::new("value")
                }],
            )
            .expect("constraints");
        cassie
            .midge
            .put_index(&IndexMeta {
                collection: collection.clone(),
                name: "analytics".to_string(),
                field: "value".to_string(),
                fields: vec!["value".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::Scalar,
                unique: false,
                options: std::collections::BTreeMap::new(),
            })
            .expect("index");
        cassie
            .midge
            .put_document(
                &collection,
                Some("row-1".to_string()),
                serde_json::json!({"value": "copy"}),
            )
            .expect("row");
    }

    async fn backup_database<R, W>(reader: &mut R, writer: &mut W) -> Vec<u8>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        writer
            .write_all(&query_frame("BACKUP DATABASE analytics TO STDOUT"))
            .await
            .expect("backup query");
        writer.flush().await.expect("flush backup query");
        assert_eq!(support::read_wire_frame(reader).await.0, b'H');
        let mut image = Vec::new();
        loop {
            let frame = support::read_wire_frame(reader).await;
            match frame.0 {
                b'd' => image.extend_from_slice(&frame.1),
                b'c' => break,
                other => panic!("unexpected backup frame {other:?}"),
            }
        }
        assert_eq!(support::read_wire_frame(reader).await.0, b'C');
        assert_eq!(support::read_wire_frame(reader).await.0, b'Z');
        image
    }

    async fn restore_database<R, W>(reader: &mut R, writer: &mut W, image: &[u8])
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        writer
            .write_all(&query_frame("RESTORE DATABASE restored FROM STDIN"))
            .await
            .expect("restore query");
        writer.flush().await.expect("flush restore query");
        assert_eq!(support::read_wire_frame(reader).await.0, b'G');
        for chunk in image.chunks(3) {
            writer
                .write_all(&copy_data_frame(chunk))
                .await
                .expect("restore data");
        }
        writer
            .write_all(&copy_done_frame())
            .await
            .expect("restore done");
        writer.flush().await.expect("flush restore data");
        assert_eq!(support::read_wire_frame(reader).await.0, b'C');
        assert_eq!(support::read_wire_frame(reader).await.0, b'Z');
    }

    fn assert_restored_database(cassie: &Cassie) {
        let collection = canonical_relation_name("restored", "public", "analytics");
        let restored = cassie
            .midge
            .get_document(&collection, "row-1")
            .expect("restored lookup")
            .expect("restored row");
        assert_eq!(restored.payload["value"], "copy");
        let metadata = cassie
            .midge
            .collection_metadata(&collection)
            .expect("restored collection metadata")
            .expect("restored collection");
        assert_eq!(
            metadata.description.as_deref(),
            Some("analytics text must remain analytics")
        );
        let constraints = cassie
            .midge
            .load_constraints(&collection)
            .expect("restored constraints");
        assert_eq!(
            constraints[0].default_value,
            Some(serde_json::json!("analytics"))
        );
        let indexes = cassie.catalog.list_indexes(&collection);
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].name, "analytics");
        assert_eq!(indexes[0].collection, collection);
    }

    #[test]
    fn should_preserve_collection_name_matching_source_database_through_pgwire_restore() {
        // Arrange
        support::use_local_storage();
        let path = data_dir("round_trip");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            seed_database_image_fixture(&cassie);

            // Act
            let server = support::spawn_server(cassie.clone()).await;
            {
                let mut socket = tokio::net::TcpStream::connect(server.addr)
                    .await
                    .expect("connect");
                let (read_half, mut write_half) = socket.split();
                let mut reader = tokio::io::BufReader::new(read_half);
                support::complete_startup(&mut reader, &mut write_half).await;
                let image = backup_database(&mut reader, &mut write_half).await;
                restore_database(&mut reader, &mut write_half, &image).await;
                write_half.shutdown().await.expect("close pgwire client");
            }
            cassie.hydrate_catalog().expect("hydrate restored catalog");

            // Assert
            assert_restored_database(&cassie);
            server.stop().await;
            tokio::task::yield_now().await;

            // Restart and assert persisted state.
            drop(cassie);
            let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
            restarted.startup().expect("restarted startup");

            // Assert
            assert_restored_database(&restarted);
            drop(restarted);
            let _ = std::fs::remove_dir_all(path);
        });
    }
}
// Formerly tests/pgwire_database_scope.rs.
mod pgwire_database_scope {
    use super::support_pgwire as pgwire_support;

    use std::time::Duration;

    use cassie::app::Cassie;
    use pgwire_support::{
        data_dir, parse_error_fields, read_wire_frame, startup_frame, use_local_storage,
    };

    fn password_frame(password: &str) -> Vec<u8> {
        pgwire_support::password_message(password)
    }

    #[test]
    fn should_report_3d000_for_missing_startup_database() {
        // Arrange
        use_local_storage();
        let path = data_dir("missing_database");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config =
                cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
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
            let startup = startup_frame("root", "missing_db");
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
                .await
                .expect("write startup");
            let (auth_tag, auth_payload) = read_wire_frame(&mut reader).await;
            assert_eq!(auth_tag, b'R');
            assert_eq!(
                i32::from_be_bytes(
                    auth_payload[..4]
                        .try_into()
                        .expect("authentication request code")
                ),
                3
            );
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &password_frame("postgres"))
                .await
                .expect("write password");
            let (tag, payload) = read_wire_frame(&mut reader).await;
            let fields = parse_error_fields(&payload);

            // Assert
            assert_eq!(tag, b'E');
            assert!(fields.contains(&('C', "3D000".to_string())));

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/pgwire_hardening.rs.
mod pgwire_hardening {
    use super::support_pgwire as pgwire;

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
        use_local_storage();
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
        use_local_storage();
        let path = data_dir("oversized_password");
        let runtime = runtime();

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "secret".to_string();
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
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
                .write_all(&startup_frame("root", "postgres"))
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
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
}

// Formerly tests/pgwire_metrics.rs.
mod pgwire_metrics {
    use super::support_pgwire as pgwire_support;

    use std::net::SocketAddr;

    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::{DataType, FieldSchema, Schema};
    use pgwire_support::{data_dir, simple_query_frame, startup_frame, use_local_storage};

    type WireFrame = (u8, Vec<u8>);
    type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

    fn password_frame(password: &str) -> Vec<u8> {
        pgwire_support::password_message(password)
    }

    async fn read_auth_frame(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    ) -> (u8, i32, Vec<u8>) {
        let mut header = [0u8; 5];
        tokio::io::AsyncReadExt::read_exact(reader, &mut header)
            .await
            .expect("read auth frame header");

        let tag = header[0];
        let len = i32::from_be_bytes(header[1..].try_into().expect("auth frame length"));
        let mut payload =
            vec![0u8; usize::try_from(len - 4).expect("non-negative auth payload length")];
        tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
            .await
            .expect("read auth frame payload");

        (tag, len, payload)
    }

    async fn read_wire_frame(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    ) -> (u8, Vec<u8>) {
        pgwire_support::read_wire_frame(reader).await
    }

    async fn read_until_ready(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    ) -> Vec<u8> {
        pgwire_support::read_until_ready(reader).await
    }

    fn seed_pgwire_metrics_collection(cassie: &Cassie) {
        let collection = canonical_relation_name("postgres", "public", "pgwire_metrics_docs");
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
        cassie.catalog.register_collection(
            &collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        cassie
            .midge
            .put_document(
                &collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
    }

    async fn spawn_pgwire_metrics_server(
        cassie: &Cassie,
        config: CassieRuntimeConfig,
    ) -> (SocketAddr, PgwireServer) {
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
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (addr, server)
    }

    async fn run_pgwire_metrics_query(addr: SocketAddr) -> Vec<WireFrame> {
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        let startup = startup_frame("root", "postgres");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
            .await
            .expect("startup write");

        let auth = read_auth_frame(&mut reader).await;
        assert_eq!(
            auth.0, b'R',
            "startup should return an authentication frame"
        );
        assert_eq!(
            i32::from_be_bytes(auth.2[0..4].try_into().expect("auth payload")),
            3,
            "startup should request a cleartext password"
        );
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &password_frame("postgres"))
            .await
            .expect("password write");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush password");
        let auth_ok = read_auth_frame(&mut reader).await;
        assert_eq!(auth_ok.0, b'R', "password should return an auth response");
        assert_eq!(
            i32::from_be_bytes(auth_ok.2[0..4].try_into().expect("auth payload")),
            0,
            "password auth should succeed"
        );
        let startup_ready = read_until_ready(&mut reader).await;
        assert_eq!(startup_ready, vec![b'I']);

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &simple_query_frame("SELECT title FROM pgwire_metrics_docs ORDER BY title"),
        )
        .await
        .expect("query write");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush");

        read_ready_frames(&mut reader).await
    }

    async fn read_ready_frames(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    ) -> Vec<WireFrame> {
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

    fn assert_pgwire_query_frames(frames: &[WireFrame]) {
        assert!(
            frames.iter().any(|frame| frame.0 == b'T'),
            "pgwire query should return a row description frame"
        );
        assert!(
            frames.iter().any(|frame| frame.0 == b'D'),
            "pgwire query should return a data row frame"
        );
        assert!(frames.iter().any(|frame| frame.0 == b'C'));
        assert!(frames.iter().any(|frame| frame.0 == b'Z'));
    }

    fn assert_pgwire_metrics(metrics: &serde_json::Value) {
        assert_eq!(
            metrics["pgwire"]["sessions_started_total"].as_u64(),
            Some(1)
        );
        assert_eq!(metrics["pgwire"]["auth_ok_total"].as_u64(), Some(1));
        assert_eq!(metrics["pgwire"]["simple_queries_total"].as_u64(), Some(1));
        assert_eq!(metrics["pgwire"]["active_sessions"].as_u64(), Some(0));
        assert_eq!(metrics["pgwire"]["prepared_statements"].as_u64(), Some(0));
    }

    #[test]
    fn should_record_pgwire_connection_metrics() {
        // Arrange
        use_local_storage();
        let path = data_dir("session_messages");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            seed_pgwire_metrics_collection(&cassie);
            let (addr, server) = spawn_pgwire_metrics_server(&cassie, config).await;

            // Act
            let frames = run_pgwire_metrics_query(addr).await;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let metrics = cassie.metrics();

            // Assert
            assert_pgwire_query_frames(&frames);
            assert_pgwire_metrics(&metrics);

            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/pgwire_simple_query.rs.
mod pgwire_simple_query {
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
        use_local_storage,
    };

    type WireFrame = (u8, Vec<u8>);
    type PgwireReader<'a> = tokio::io::BufReader<tokio::net::tcp::ReadHalf<'a>>;
    type PgwireWriter<'a> = tokio::net::tcp::WriteHalf<'a>;
    type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

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

    #[test]
    fn should_copy_csv_from_stdin_rows() {
        // Arrange
        use_local_storage();
        let path = data_dir("copy_stdin");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            seed_copy_collection(&cassie);

            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            start_pgwire_session(&mut reader, &mut write_half).await;
            request_copy_from_stdin(&mut reader, &mut write_half).await;
            let (complete, ready) = send_copy_rows(&mut reader, &mut write_half).await;

            // Assert
            assert_copy_complete_frames(&complete, &ready);
            let select_frames = write_simple_query_and_read_frames(
                &mut reader,
                &mut write_half,
                "SELECT title, score FROM simple_copy_docs ORDER BY score",
            )
            .await;
            assert_copy_select_frames(&select_frames);

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_binary_simple_query_return_backend_frames() {
        // Arrange
        use_local_storage();
        let path = data_dir("success");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            seed_simple_query_collection(&cassie);

            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            start_pgwire_session(&mut reader, &mut write_half).await;
            let frames = write_simple_query_and_read_frames(
                &mut reader,
                &mut write_half,
                "SELECT title FROM simple_query_docs ORDER BY title",
            )
            .await;

            // Assert
            assert_simple_query_backend_frames(&frames);

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_return_row_description_for_empty_simple_query_result() {
        // Arrange
        use_local_storage();
        let path = data_dir("empty_result");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();

            let collection =
                canonical_relation_name("postgres", "public", "simple_query_empty_docs");
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

            start_pgwire_session(&mut reader, &mut write_half).await;

            // Act
            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                &simple_query_frame(
                    "SELECT title FROM simple_query_empty_docs WHERE title = 'missing'",
                ),
            )
            .await
            .expect("write query");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush query");

            let mut frames = Vec::new();
            loop {
                let frame = read_wire_frame(&mut reader).await;
                let tag = frame.0;
                frames.push(frame);
                if tag == b'Z' {
                    break;
                }
            }

            // Assert
            assert_eq!(frames.len(), 3);
            assert_eq!(frames[0].0, b'T', "empty select should describe columns");
            assert_eq!(frames[1].0, b'C', "empty select should complete command");
            assert_eq!(frames[2].0, b'Z', "empty select should return ready");
            assert_eq!(parse_row_description(&frames[0].1)[0].0, "title");
            assert_eq!(frames[2].1, vec![b'I']);

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_recover_ready_after_simple_query_error() {
        // Arrange
        use_local_storage();
        let path = data_dir("error");
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
                &simple_query_frame("SELECT title FROM missing_simple_query_table"),
            )
            .await
            .expect("write query");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush query");

            let error = read_wire_frame(&mut reader).await;
            let ready = read_wire_frame(&mut reader).await;

            // Assert
            assert_eq!(error.0, b'E', "query failure should return an error frame");
            assert_eq!(
                ready.0, b'Z',
                "query failure should still return ready-for-query"
            );
            let error_fields = parse_error_fields(&error.1);
            assert_eq!(
                error_fields
                    .iter()
                    .find(|(field, _)| *field == 'C')
                    .map(|(_, value)| value.as_str()),
                Some("42P01"),
                "missing table should use undefined table SQLSTATE"
            );
            assert_eq!(
                error_fields
                    .iter()
                    .find(|(field, _)| *field == 't')
                    .map(|(_, value)| value.as_str()),
                Some("missing_simple_query_table"),
                "missing table should include table metadata"
            );
            assert!(
                error_fields.iter().any(|(field, value)| {
                    *field == 'M' && value.contains("missing_simple_query_table")
                }),
                "error response should mention the missing table"
            );
            assert_eq!(ready.1, vec![b'I']);

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_recover_pgwire_after_oversized_sql_resource_limit() {
        // Arrange
        use_local_storage();
        let path = data_dir("sql-resource-limit");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = CassieRuntimeConfig {
                password: "postgres".to_string(),
                ..CassieRuntimeConfig::default()
            };
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
            cassie.startup().expect("startup");
            let (addr, server) = spawn_pgwire_server_with_config(&cassie, config).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            start_pgwire_session(&mut reader, &mut write_half).await;

            // Act
            let oversized = "x".repeat(1024 * 1024 + 1);
            let rejected =
                write_simple_query_and_read_frames(&mut reader, &mut write_half, &oversized).await;
            let recovered = write_simple_query_and_read_frames(
                &mut reader,
                &mut write_half,
                "SELECT version()",
            )
            .await;

            // Assert
            assert_eq!(rejected[0].0, b'E');
            assert_eq!(
                error_field(&parse_error_fields(&rejected[0].1), 'C'),
                Some("54000")
            );
            assert_eq!(rejected.last().map(|frame| frame.0), Some(b'Z'));
            assert!(recovered.iter().all(|frame| frame.0 != b'E'));
            assert_eq!(recovered.last().map(|frame| frame.0), Some(b'Z'));

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_retryable_storage_error_with_cannot_connect_now_sqlstate() {
        // Arrange
        use_local_storage();
        let path = data_dir("retryable_storage");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            start_pgwire_session(&mut reader, &mut write_half).await;
            cassie::pgwire::connection::arm_next_pgwire_blocking_retryable_failure_for_test(
                &cassie,
                "pgwire blocking boundary test retryable failure",
            );
            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                &simple_query_frame("SELECT version()"),
            )
            .await
            .expect("write query");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush query");
            let error = read_wire_frame(&mut reader).await;
            let ready = read_wire_frame(&mut reader).await;
            let error_fields = parse_error_fields(&error.1);

            // Assert
            assert_eq!(error.0, b'E');
            assert_eq!(ready.0, b'Z');
            assert_eq!(error_field(&error_fields, 'C'), Some("57P03"));
            assert_eq!(
            error_field(&error_fields, 'M'),
            Some("temporary storage unavailable: pgwire blocking boundary test retryable failure")
        );
            assert_eq!(ready.1, vec![b'I']);

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_bigint_out_of_range_with_sqlstate_22003() {
        // Arrange
        use_local_storage();
        let path = data_dir("bigint_out_of_range");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("startup");
            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            start_pgwire_session(&mut reader, &mut write_half).await;
            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                &simple_query_frame("SELECT 9223372036854775807 + 1"),
            )
            .await
            .expect("write query");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush query");
            let error = read_wire_frame(&mut reader).await;
            let ready = read_wire_frame(&mut reader).await;
            let error_fields = parse_error_fields(&error.1);

            // Assert
            assert_eq!(error.0, b'E');
            assert_eq!(ready.0, b'Z');
            assert_eq!(error_field(&error_fields, 'C'), Some("22003"));
            assert_eq!(error_field(&error_fields, 'M'), Some("bigint out of range"));

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_division_by_zero_with_sqlstate_22012() {
        // Arrange
        use_local_storage();
        let path = data_dir("division_by_zero");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE division_by_zero (score INT)",
                    vec![],
                )
                .expect("create table");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO division_by_zero (score) VALUES (4)",
                    vec![],
                )
                .expect("seed row");
            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            start_pgwire_session(&mut reader, &mut write_half).await;
            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                &simple_query_frame("SELECT score FROM division_by_zero WHERE (score / 0) = 1"),
            )
            .await
            .expect("write query");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush query");
            let error = read_wire_frame(&mut reader).await;
            let ready = read_wire_frame(&mut reader).await;
            let error_fields = parse_error_fields(&error.1);

            // Assert
            assert_eq!(error.0, b'E');
            assert_eq!(ready.0, b'Z');
            assert_eq!(error_field(&error_fields, 'C'), Some("22012"));
            assert_eq!(error_field(&error_fields, 'M'), Some("division by zero"));
            assert_eq!(ready.1, vec![b'I']);

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_deadline_exceeded_with_query_canceled_sqlstate() {
        // Arrange
        use_local_storage();
        let path = data_dir("query_deadline");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.query_timeout_ms = 1;
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().expect("startup");
            seed_simple_query_collection(&cassie);
            let (addr, server) = spawn_pgwire_server_with_config(&cassie, config).await;
            let mut socket = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            start_pgwire_session(&mut reader, &mut write_half).await;
            let sql = format!(
                "{}SELECT title FROM simple_query_docs",
                " ".repeat(1_000_000)
            );
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &simple_query_frame(&sql))
                .await
                .expect("write query");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush query");
            let error = read_wire_frame(&mut reader).await;
            let ready = read_wire_frame(&mut reader).await;
            let error_fields = parse_error_fields(&error.1);

            // Assert
            assert_eq!(error.0, b'E');
            assert_eq!(ready.0, b'Z');
            assert_eq!(
                error_fields
                    .iter()
                    .find(|(field, _)| *field == 'C')
                    .map(|(_, value)| value.as_str()),
                Some("57014"),
                "unexpected pgwire error fields: {error_fields:?}",
            );
            assert!(
                error_fields.iter().any(|(field, value)| {
                    *field == 'M' && value.contains("query timeout exceeded")
                }),
                "deadline error should mention query timeout"
            );

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_align_wildcard_data_rows_with_row_description_when_table_declares_id() {
        // Arrange
        use_local_storage();
        let path = data_dir("simple-wildcard-declared-id");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE simple_wildcard_id_docs (id TEXT, title TEXT)",
                "INSERT INTO simple_wildcard_id_docs (id, title) VALUES ('a', 'x')",
                "CREATE TABLE simple_wildcard_upper_id_docs (title TEXT, ID TEXT)",
                "INSERT INTO simple_wildcard_upper_id_docs (title, ID) VALUES ('y', 'b')",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .unwrap_or_else(|error| panic!("{sql}: {error}"));
            }
            let (addr, server) = spawn_pgwire_server(&cassie).await;
            let mut socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
            let (read_half, mut writer) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            start_pgwire_session(&mut reader, &mut writer).await;
            let id_frames = write_simple_query_and_read_frames(
                &mut reader,
                &mut writer,
                "SELECT id FROM simple_wildcard_id_docs",
            )
            .await;
            let row_id = pgwire_support::data_rows(&id_frames)[0][0].clone();

            // Act
            let mut results = Vec::new();
            for sql in [
                "SELECT * FROM simple_wildcard_id_docs",
                "SELECT * FROM simple_wildcard_id_docs WHERE title = 'x'",
                "SELECT * FROM simple_wildcard_id_docs ORDER BY title",
                "SELECT * FROM simple_wildcard_id_docs LIMIT 1",
            ] {
                let frames =
                    write_simple_query_and_read_frames(&mut reader, &mut writer, sql).await;
                results.push((sql, frames));
            }
            let upper_frames = write_simple_query_and_read_frames(
                &mut reader,
                &mut writer,
                "SELECT * FROM simple_wildcard_upper_id_docs",
            )
            .await;

            // Assert
            assert!(row_id.is_some(), "row id should be returned");
            for (sql, frames) in results {
                assert!(
                    frames.iter().all(|(tag, _)| *tag != b'E'),
                    "{sql} should succeed"
                );
                assert_eq!(
                    pgwire_support::row_description_names(&frames),
                    vec!["id".to_string(), "title".to_string()],
                    "{sql} row description"
                );
                assert_eq!(
                    pgwire_support::data_rows(&frames),
                    vec![vec![row_id.clone(), Some("x".to_string())]],
                    "{sql} data row values"
                );
            }
            // `simple_wildcard_upper_id_docs` declares its own `ID` field
            // (case-insensitively still "id"), so wildcard output preserves
            // the real schema field order and values instead of
            // synthesizing an internal-identity `id` column.
            assert_eq!(
                pgwire_support::row_description_names(&upper_frames),
                vec!["title".to_string(), "ID".to_string()]
            );
            let upper_rows = pgwire_support::data_rows(&upper_frames);
            assert_eq!(upper_rows.len(), 1);
            assert_eq!(upper_rows[0].len(), 2);
            assert_eq!(upper_rows[0][0], Some("y".to_string()));
            assert_eq!(upper_rows[0][1], Some("b".to_string()));

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/pgwire_startup.rs.
mod pgwire_startup {
    use super::support_pgwire as pgwire_support;

    use std::time::Duration;

    use cassie::app::Cassie;

    use pgwire_support::{
        data_dir, parse_error_fields, password_message, startup_frame, use_local_storage,
    };

    const TEST_PASSWORD: &str = "cassie-pgwire-startup-password";

    fn authenticated_config() -> cassie::config::CassieRuntimeConfig {
        cassie::config::CassieRuntimeConfig {
            password: TEST_PASSWORD.to_string(),
            ..cassie::config::CassieRuntimeConfig::default()
        }
    }

    fn startup_frame_with_params(user: &str, database: &str, params: &[(&str, &str)]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
        payload.extend_from_slice(b"user\0");
        payload.extend_from_slice(user.as_bytes());
        payload.push(0);
        payload.extend_from_slice(b"database\0");
        payload.extend_from_slice(database.as_bytes());
        payload.push(0);
        for (key, value) in params {
            payload.extend_from_slice(key.as_bytes());
            payload.push(0);
            payload.extend_from_slice(value.as_bytes());
            payload.push(0);
        }
        payload.push(0);

        let mut frame = Vec::new();
        frame.extend_from_slice(
            &i32::try_from(payload.len() + 4)
                .expect("startup payload size must fit into i32")
                .to_be_bytes(),
        );
        frame.extend_from_slice(&payload);
        frame
    }

    fn ssl_request_frame() -> Vec<u8> {
        let mut frame = Vec::new();
        frame.extend_from_slice(&8_i32.to_be_bytes());
        frame.extend_from_slice(&80_877_103_i32.to_be_bytes());
        frame
    }

    async fn read_wire_frame(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    ) -> (u8, i32, Vec<u8>) {
        let (tag, payload) = pgwire_support::read_wire_frame(reader).await;
        let len = i32::try_from(payload.len() + 4).expect("frame length must fit into i32");
        (tag, len, payload)
    }

    async fn complete_password_authentication(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
        writer: &mut tokio::net::tcp::WriteHalf<'_>,
    ) -> (i32, i32) {
        let (challenge_tag, _, challenge_payload) = read_wire_frame(reader).await;
        assert_eq!(challenge_tag, b'R', "password challenge should use R tag");
        let challenge = i32::from_be_bytes(
            challenge_payload[..4]
                .try_into()
                .expect("authentication challenge code"),
        );
        tokio::io::AsyncWriteExt::write_all(writer, &password_message(TEST_PASSWORD))
            .await
            .expect("write password");
        let (authenticated_tag, _, authenticated_payload) = read_wire_frame(reader).await;
        assert_eq!(authenticated_tag, b'R', "authentication should use R tag");
        let authenticated = i32::from_be_bytes(
            authenticated_payload[..4]
                .try_into()
                .expect("authentication completion code"),
        );
        (challenge, authenticated)
    }

    async fn read_startup_error(addr: std::net::SocketAddr, startup: &[u8]) -> Vec<(char, String)> {
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        tokio::io::AsyncWriteExt::write_all(&mut write_half, startup)
            .await
            .expect("write startup");
        let (tag, _, payload) = read_wire_frame(&mut reader).await;
        assert_eq!(tag, b'E', "invalid startup should return an error frame");
        parse_error_fields(&payload)
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

    fn parse_parameter_status(payload: &[u8]) -> (String, String) {
        let mut cursor = 0usize;
        let key = read_cstring(payload, &mut cursor);
        let value = read_cstring(payload, &mut cursor);
        (key, value)
    }

    #[test]
    fn should_validate_every_startup_parameter_regardless_of_user_value() {
        // Arrange
        use_local_storage();
        let path = data_dir("startup_parameter_validation");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = authenticated_config();
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
            let cases = [
                ("", "", vec![], "invalid startup option 'database'"),
                (
                    "",
                    "postgres",
                    vec![("replication", "database")],
                    "unsupported startup option: replication",
                ),
                (
                    "",
                    "postgres",
                    vec![("client_encoding", "LATIN1")],
                    "invalid parameter value: invalid value for parameter \"client_encoding\": \"LATIN1\"",
                ),
                ("root", "", vec![], "invalid startup option 'database'"),
                (
                    "root",
                    "postgres",
                    vec![("replication", "database")],
                    "unsupported startup option: replication",
                ),
                (
                    "root",
                    "postgres",
                    vec![("client_encoding", "LATIN1")],
                    "invalid parameter value: invalid value for parameter \"client_encoding\": \"LATIN1\"",
                ),
            ];

            // Act
            let mut observed = Vec::new();
            for (user, database, params, expected) in cases {
                let startup = startup_frame_with_params(user, database, &params);
                let fields = read_startup_error(addr, &startup).await;
                let message = fields
                    .iter()
                    .find(|(field, _)| *field == 'M')
                    .map(|(_, value)| value.clone());
                observed.push((message, expected));
            }

            // Assert
            for (actual, expected) in observed {
                assert_eq!(actual.as_deref(), Some(expected));
            }

            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_support_binary_startup_with_password_authentication() {
        // Arrange
        use_local_storage();
        let path = data_dir("auth_ok");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = authenticated_config();
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
            let startup = startup_frame("root", "postgres");
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
                .await
                .expect("write startup");
            let authentication =
                complete_password_authentication(&mut reader, &mut write_half).await;

            // Assert
            assert_eq!(authentication, (3, 0));

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_emit_startup_parameter_statuses_after_password_authentication() {
        // Arrange
        use_local_storage();
        let path = data_dir("parameter_statuses");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = authenticated_config();
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
            let startup = startup_frame("root", "postgres");
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
                .await
                .expect("write startup");
            let authentication =
                complete_password_authentication(&mut reader, &mut write_half).await;
            let mut statuses = Vec::new();
            loop {
                let (tag, _len, payload) = read_wire_frame(&mut reader).await;
                if tag == b'Z' {
                    break;
                }
                if tag == b'S' {
                    statuses.push(parse_parameter_status(&payload));
                }
            }

            // Assert
            assert_eq!(authentication, (3, 0));
            assert!(statuses.contains(&("server_version".to_string(), "16.0".to_string())));
            assert!(statuses.contains(&("server_encoding".to_string(), "UTF8".to_string())));
            assert!(statuses.contains(&("client_encoding".to_string(), "UTF8".to_string())));
            assert!(statuses.contains(&("DateStyle".to_string(), "ISO, MDY".to_string())));
            assert!(statuses.contains(&("integer_datetimes".to_string(), "on".to_string())));
            assert!(statuses.contains(&("TimeZone".to_string(), "UTC".to_string())));
            assert!(
                statuses.contains(&("standard_conforming_strings".to_string(), "on".to_string()))
            );

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_emit_backend_key_data_after_authentication() {
        // Arrange
        use_local_storage();
        let path = data_dir("backend_key_data");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = authenticated_config();
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
            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                &startup_frame("root", "postgres"),
            )
            .await
            .expect("write startup");
            complete_password_authentication(&mut reader, &mut write_half).await;

            // Act
            let mut backend_key = None;
            loop {
                let (tag, _, payload) = read_wire_frame(&mut reader).await;
                if tag == b'K' {
                    backend_key = Some(payload);
                }
                if tag == b'Z' {
                    break;
                }
            }

            // Assert
            let payload = backend_key.expect("backend key data");
            assert_eq!(payload.len(), 8);
            assert_ne!(i32::from_be_bytes(payload[..4].try_into().unwrap()), 0);
            assert_ne!(i32::from_be_bytes(payload[4..].try_into().unwrap()), 0);

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_accept_libpq_startup_hints_with_password_authentication() {
        // Arrange
        use_local_storage();
        let path = data_dir("libpq_hints");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = authenticated_config();
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
            let startup = startup_frame_with_params(
                "root",
                "postgres",
                &[
                    ("_pq_.libpq_version", "170000"),
                    ("application_name", "sqlalchemy"),
                ],
            );
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
                .await
                .expect("write startup");
            let authentication =
                complete_password_authentication(&mut reader, &mut write_half).await;

            // Assert
            assert_eq!(authentication, (3, 0));

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_return_not_supported_for_ssl_request() {
        // Arrange
        use_local_storage();
        let path = data_dir("ssl_request");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = authenticated_config();
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

            // Act
            let request = ssl_request_frame();
            tokio::io::AsyncWriteExt::write_all(&mut socket, &request)
                .await
                .expect("write ssl request");
            let mut reply = [0u8; 1];
            tokio::io::AsyncReadExt::read_exact(&mut socket, &mut reply)
                .await
                .expect("read ssl response");

            // Assert
            assert_eq!(reply[0], b'N', "SSL request should be explicitly declined");

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_error_when_password_does_not_match_for_cleartext_auth() {
        // Arrange
        use_local_storage();
        let path = data_dir("auth_failure");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = authenticated_config();
            config.password = "correct-password".to_string();
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
            let startup = startup_frame("root", "postgres");
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
                .await
                .expect("write startup");
            let (tag, _len, _payload) = read_wire_frame(&mut reader).await;
            assert_eq!(tag, b'R', "password challenge should use R tag");

            // password challenge for cleartext auth expects tag "R" status 3 then password message
            let payload = password_message("wrong-password");
            tokio::io::AsyncWriteExt::write_all(&mut write_half, &payload)
                .await
                .expect("write password");
            let (error_tag, _error_len, error_payload) = read_wire_frame(&mut reader).await;

            // Assert
            assert_eq!(
                error_tag, b'E',
                "auth failure should be returned as an error frame"
            );
            let error_fields = parse_error_fields(&error_payload);
            assert_eq!(
                error_fields
                    .iter()
                    .find(|(field, _)| *field == 'C')
                    .map(|(_, value)| value.as_str()),
                Some("28000"),
                "error response should include SQL state"
            );
            assert_eq!(
                error_fields
                    .iter()
                    .find(|(field, _)| *field == 'S')
                    .map(|(_, value)| value.as_str()),
                Some("FATAL"),
                "auth failure should be a fatal error"
            );

            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}
// Formerly tests/pgwire_tls.rs.
mod pgwire_tls {
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::rustls::pki_types::ServerName;
    use tokio_rustls::rustls::{ClientConfig, RootCertStore};
    use tokio_rustls::TlsConnector;

    use super::support_pgwire as support;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    async fn connect_tls(
        address: std::net::SocketAddr,
        certificate: rustls::pki_types::CertificateDer<'static>,
    ) -> tokio_rustls::client::TlsStream<tokio::net::TcpStream> {
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        socket
            .write_all(&[0, 0, 0])
            .await
            .expect("fragmented SSLRequest prefix");
        tokio::task::yield_now().await;
        socket
            .write_all(&[8, 4, 210, 22, 47])
            .await
            .expect("fragmented SSLRequest suffix");
        let mut response = [0_u8; 1];
        socket
            .read_exact(&mut response)
            .await
            .expect("SSL response");
        assert_eq!(response, *b"S");
        let mut roots = RootCertStore::empty();
        roots.add(certificate).expect("root certificate");
        let client = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        TlsConnector::from(Arc::new(client))
            .connect(
                ServerName::try_from("localhost")
                    .expect("server name")
                    .to_owned(),
                socket,
            )
            .await
            .expect("TLS handshake")
    }

    #[test]
    fn should_execute_pgwire_query_over_negotiated_tls() {
        // Arrange
        support::use_local_storage();
        let data_path = support::data_dir("tls-query");
        let tls_path =
            std::env::temp_dir().join(format!("cassie-pgwire-tls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tls_path).expect("TLS directory");
        let certificate_path = tls_path.join("cert.pem");
        let key_path = tls_path.join("key.pem");
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("TLS identity");
        std::fs::write(&certificate_path, identity.cert.pem()).expect("certificate fixture");
        std::fs::write(&key_path, identity.signing_key.serialize_pem()).expect("key fixture");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_path).expect("cassie");
            cassie.startup().expect("startup");
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let address = listener.local_addr().expect("listener address");
            drop(listener);
            let config = CassieRuntimeConfig {
                password: "postgres".to_string(),
                pgwire_tls_cert_file: Some(certificate_path.to_string_lossy().to_string()),
                pgwire_tls_key_file: Some(key_path.to_string_lossy().to_string()),
                ..CassieRuntimeConfig::default()
            };
            let shutdown = Arc::new(tokio::sync::Notify::new());
            let server = tokio::spawn(cassie::pgwire::server::run_with_shutdown(
                address.to_string(),
                Arc::new(cassie),
                config,
                shutdown.clone(),
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;

            let tls = connect_tls(address, identity.cert.der().clone()).await;
            let (mut reader, mut writer) = tokio::io::split(tls);

            // Act
            support::complete_startup(&mut reader, &mut writer).await;
            writer
                .write_all(&support::simple_query_frame("SELECT 1 AS value"))
                .await
                .expect("query frame");
            let query = support::read_frames_until_ready(&mut reader).await;

            // Assert
            assert!(query.iter().any(|(tag, _)| *tag == b'D'));
            assert_eq!(
                query.last().map(|frame| frame.1.as_slice()),
                Some(b"I".as_slice())
            );

            shutdown.notify_waiters();
            server.await.expect("server task").expect("server shutdown");
        });

        let _ = std::fs::remove_dir_all(data_path);
        let _ = std::fs::remove_dir_all(tls_path);
    }

    #[test]
    fn should_preserve_pgwire_cancel_protocol_over_tls() {
        // Arrange
        support::use_local_storage();
        let data_path = support::data_dir("tls-cancel");
        let tls_path =
            std::env::temp_dir().join(format!("cassie-pgwire-tls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tls_path).expect("TLS directory");
        let certificate_path = tls_path.join("cert.pem");
        let key_path = tls_path.join("key.pem");
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("TLS identity");
        std::fs::write(&certificate_path, identity.cert.pem()).expect("certificate fixture");
        std::fs::write(&key_path, identity.signing_key.serialize_pem()).expect("key fixture");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_path).expect("cassie");
            cassie.startup().expect("startup");
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let address = listener.local_addr().expect("listener address");
            drop(listener);
            let config = CassieRuntimeConfig {
                password: "postgres".to_string(),
                pgwire_tls_cert_file: Some(certificate_path.to_string_lossy().to_string()),
                pgwire_tls_key_file: Some(key_path.to_string_lossy().to_string()),
                ..CassieRuntimeConfig::default()
            };
            let server = tokio::spawn(cassie::pgwire::server::run(
                address.to_string(),
                Arc::new(cassie),
                config,
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;
            let query_tls = connect_tls(address, identity.cert.der().clone()).await;
            let (mut query_reader, mut query_writer) = tokio::io::split(query_tls);
            let (process_id, secret_key) =
                support::complete_startup_with_backend_key(&mut query_reader, &mut query_writer)
                    .await;

            // Act
            let mut cancel_tls = connect_tls(address, identity.cert.der().clone()).await;
            cancel_tls
                .write_all(&support::cancel_request_frame(process_id, secret_key))
                .await
                .expect("TLS cancel request");
            cancel_tls.shutdown().await.expect("close cancel request");
            query_writer
                .write_all(&support::simple_query_frame("SELECT 1"))
                .await
                .expect("query after idle cancel");
            let frames = support::read_frames_until_ready(&mut query_reader).await;

            // Assert
            assert!(frames.iter().all(|(tag, _)| *tag != b'E'));
            assert!(frames.iter().any(|(tag, _)| *tag == b'D'));

            server.abort();
            let _ = server.await;
        });

        let _ = std::fs::remove_dir_all(data_path);
        let _ = std::fs::remove_dir_all(tls_path);
    }

    #[test]
    fn should_reject_invalid_pgwire_tls_identity_before_listening() {
        // Arrange
        support::use_local_storage();
        let data_path = support::data_dir("tls-invalid-identity");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_path).expect("cassie");
            cassie.startup().expect("startup");
            let config = CassieRuntimeConfig {
                pgwire_tls_cert_file: Some("/tmp/cassie-missing-pgwire-cert.pem".to_string()),
                pgwire_tls_key_file: Some("/tmp/cassie-missing-pgwire-key.pem".to_string()),
                ..CassieRuntimeConfig::default()
            };

            // Act
            let error =
                cassie::pgwire::server::run("127.0.0.1:0".to_string(), Arc::new(cassie), config)
                    .await
                    .expect_err("invalid TLS identity must fail startup");

            // Assert
            assert!(error.to_string().contains("pgwire TLS"));
            assert!(error.to_string().contains("file not found"));
        });

        let _ = std::fs::remove_dir_all(data_path);
    }
}

// Formerly tests/pgwire_transaction_semantics.rs.
mod pgwire_transaction_semantics {
    use cassie::app::Cassie;

    use super::support_pgwire as wire;
    use super::support_sql as sql;

    fn simple_query_frame(query: &str) -> Vec<u8> {
        let mut payload = query.as_bytes().to_vec();
        payload.push(0);
        let mut frame = vec![b'Q'];
        frame.extend_from_slice(
            &i32::try_from(payload.len() + 4)
                .expect("simple query payload size must fit into i32")
                .to_be_bytes(),
        );
        frame.extend_from_slice(&payload);
        frame
    }

    fn error_field(fields: &[(char, String)], tag: char) -> Option<&str> {
        fields
            .iter()
            .find(|(field, _)| *field == tag)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn should_report_transaction_semantics_sqlstate_through_pgwire() {
        // Arrange
        sql::use_local_storage();
        let path = sql::data_dir("pgwire_transaction_semantics");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("startup");
            let setup = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &setup,
                    "CREATE TABLE pgwire_transaction_semantics_source (title TEXT)",
                    vec![],
                )
                .expect("create source table");
            let server = wire::spawn_server(cassie).await;
            let socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect pgwire");
            let (mut reader, mut writer) = tokio::io::split(socket);
            wire::complete_startup(&mut reader, &mut writer).await;

            // Act
            wire::write_frames(
                &mut writer,
                vec![simple_query_frame("BEGIN ISOLATION LEVEL SERIALIZABLE")],
            )
            .await;
            let frames = wire::read_frames_until_ready(&mut reader).await;

            // Assert
            let error = frames
                .iter()
                .find(|(tag, _)| *tag == b'E')
                .expect("transaction isolation error");
            let fields = wire::parse_error_fields(&error.1);
            assert_eq!(error_field(&fields, 'C'), Some("0A000"));
            assert_eq!(frames.last().expect("idle ready").1, vec![b'I']);

            wire::write_frames(&mut writer, vec![simple_query_frame("BEGIN")]).await;
            let begin_frames = wire::read_frames_until_ready(&mut reader).await;
            assert_eq!(begin_frames.last().expect("active ready").1, vec![b'T']);

            wire::write_frames(
                &mut writer,
                vec![simple_query_frame(
                    "CREATE TABLE pgwire_transaction_semantics_rejected (value TEXT)",
                )],
            )
            .await;
            let ddl_frames = wire::read_frames_until_ready(&mut reader).await;
            let ddl_error = ddl_frames
                .iter()
                .find(|(tag, _)| *tag == b'E')
                .expect("DDL transaction error");
            let ddl_fields = wire::parse_error_fields(&ddl_error.1);
            assert_eq!(error_field(&ddl_fields, 'C'), Some("0A000"));
            assert_eq!(ddl_frames.last().expect("failed ready").1, vec![b'E']);

            wire::write_frames(&mut writer, vec![simple_query_frame("ROLLBACK")]).await;
            let rollback_frames = wire::read_frames_until_ready(&mut reader).await;
            assert_eq!(
                rollback_frames.last().expect("rollback ready").1,
                vec![b'I']
            );
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/pgwire_transaction_staging.rs.
mod pgwire_transaction_staging {
    use std::time::Duration;

    use cassie::app::Cassie;

    use super::support_pgwire as wire;
    use super::support_sql as sql;

    fn simple_query_frame(query: &str) -> Vec<u8> {
        let mut payload = query.as_bytes().to_vec();
        payload.push(0);
        let mut frame = vec![b'Q'];
        frame.extend_from_slice(
            &i32::try_from(payload.len() + 4)
                .expect("simple query payload size must fit into i32")
                .to_be_bytes(),
        );
        frame.extend_from_slice(&payload);
        frame
    }

    #[test]
    fn should_commit_multi_collection_staging_with_transaction_ready_status() {
        // Arrange
        sql::use_local_storage();
        let path = sql::data_dir("pgwire_transaction_staging");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("startup");
            let setup = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &setup,
                    "CREATE TABLE pgwire_stage_a (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .expect("create first collection");
            cassie
                .execute_sql(
                    &setup,
                    "CREATE TABLE pgwire_stage_b (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .expect("create second collection");
            let server = wire::spawn_server(cassie).await;
            let socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect pgwire");
            let (mut reader, mut writer) = tokio::io::split(socket);
            wire::complete_startup(&mut reader, &mut writer).await;

            wire::write_frames(&mut writer, vec![simple_query_frame("BEGIN")]).await;
            let begin_frames = wire::read_frames_until_ready(&mut reader).await;

            wire::write_frames(
                &mut writer,
                vec![simple_query_frame(
                    "INSERT INTO pgwire_stage_a (id, title) VALUES (1, 'alpha')",
                )],
            )
            .await;
            let first_frames = wire::read_frames_until_ready(&mut reader).await;

            // Act
            wire::write_frames(
                &mut writer,
                vec![simple_query_frame(
                    "INSERT INTO pgwire_stage_b (id, title) VALUES (1, 'beta')",
                )],
            )
            .await;
            let second_frames = wire::read_frames_until_ready(&mut reader).await;

            wire::write_frames(&mut writer, vec![simple_query_frame("COMMIT")]).await;
            let commit_frames = wire::read_frames_until_ready(&mut reader).await;

            // Assert
            assert_eq!(begin_frames.last().expect("begin ready").1, vec![b'T']);
            assert_eq!(first_frames.last().expect("first ready").1, vec![b'T']);
            assert!(!second_frames.iter().any(|(tag, _)| *tag == b'E'));
            assert_eq!(second_frames.last().expect("second ready").1, vec![b'T']);
            assert_eq!(commit_frames.last().expect("commit ready").1, vec![b'I']);
            assert_eq!(
                setup.transaction_status(),
                "idle",
                "setup session remains independent"
            );

            server.stop().await;
            tokio::time::sleep(Duration::from_millis(1)).await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}
