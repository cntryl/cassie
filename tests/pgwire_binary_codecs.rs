use cassie::app::Cassie;
use cassie::types::{Value, Vector};

#[path = "support/pgwire.rs"]
mod support;

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
fn should_encode_binary_result_codecs_with_exact_bytes() {
    // Arrange
    support::with_fallback();
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
    support::with_fallback();
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
    support::with_fallback();
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
    support::with_fallback();
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
    support::with_fallback();
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
            support::parse_frame_with_types("invalid_array_length_stmt", "SELECT $1", &[34_023]),
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
    support::with_fallback();
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
