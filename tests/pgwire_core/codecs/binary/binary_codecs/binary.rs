#[test]
fn should_encode_binary_result_codecs_with_exact_bytes() {
    // Arrange
    let path = support::data_dir("binary-result-codecs");

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
    let path = support::data_dir("binary-parameter-codecs");

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
                Some("2000-01-02 00:00:01.000003".to_string()),
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
    let path = support::data_dir("binary-unsupported-codecs");

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
    let path = support::data_dir("binary-vector-array-parameters");

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
                &[100_002, 1_007],
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
            vec![Some("[1.5,-2.0]".to_string()), Some("{7,8,9}".to_string())]
        );

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_store_empty_array_given_zero_dimension_binary_parameter() {
    // Arrange
    let path = support::data_dir("binary-empty-array-bind");
    let empty_array = empty_int_array_body();

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE empty_arrays (id TEXT, vals INT[])",
                vec![],
            )
            .expect("create table");

        // Act
        let (frames, server) = start_extended_query(
            cassie.clone(),
            support::parse_frame_with_types(
                "empty_array_insert",
                "INSERT INTO empty_arrays (id, vals) VALUES ('e', $1)",
                &[1_007],
            ),
            support::bind_frame_with_formats(
                "empty_array_insert_portal",
                "empty_array_insert",
                &[1],
                &[Some(&empty_array)],
                &[],
            ),
            support::execute_frame("empty_array_insert_portal"),
        )
        .await;

        // Assert
        assert!(frames.iter().all(|frame| frame.0 != b'E'));
        let stored = cassie
            .execute_sql(&session, "SELECT vals FROM empty_arrays", vec![])
            .expect("select stored array");
        assert_eq!(
            stored.rows(),
            vec![vec![Value::Array(cassie::types::SqlArray::empty())]]
        );

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_read_wide_vector_columns_in_binary_format_under_vector_oids() {
    // Arrange
    let path = support::data_dir("binary-wide-vectors");
    let dimensions = [1025_usize, 1536];

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for dimension in dimensions {
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE wide_vectors_{dimension} (e VECTOR({dimension}))"),
                    vec![],
                )
                .expect("create wide vector table");
            cassie
                .execute_sql(
                    &session,
                    &format!("INSERT INTO wide_vectors_{dimension} (e) VALUES ($1)"),
                    vec![Value::Vector(Vector::new(vec![0.25; dimension]))],
                )
                .expect("insert wide vector");
        }

        // Act
        let mut observed = Vec::new();
        for dimension in dimensions {
            let (frames, server) = start_extended_query(
                cassie.clone(),
                support::parse_frame(
                    "wide_vector_stmt",
                    &format!("SELECT e FROM wide_vectors_{dimension}"),
                ),
                support::bind_frame_with_formats(
                    "wide_vector_portal",
                    "wide_vector_stmt",
                    &[],
                    &[],
                    &[1],
                ),
                support::execute_frame("wide_vector_portal"),
            )
            .await;
            server.stop().await;
            let oid = frames
                .iter()
                .find(|frame| frame.0 == b'T')
                .map(|frame| support::parse_row_description(&frame.1)[0].type_oid);
            let body_lengths = frames
                .iter()
                .filter(|frame| frame.0 == b'D')
                .map(|frame| read_binary_row(&frame.1)[0].as_ref().map(Vec::len))
                .collect::<Vec<_>>();
            observed.push((
                oid.is_some_and(|oid| !(34_000..44_000).contains(&oid)),
                body_lengths,
            ));
        }

        // Assert
        assert_eq!(
            observed,
            dimensions
                .iter()
                .map(|dimension| (true, vec![Some(4 + 4 * dimension)]))
                .collect::<Vec<_>>()
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_encode_aggregated_array_columns_in_binary_format() {
    // Arrange
    let path = support::data_dir("binary-grouped-arrays");
    let queries = [
        "SELECT tags FROM grouped_tags",
        "SELECT tags FROM grouped_tags GROUP BY tags",
        "SELECT MIN(tags) FROM grouped_tags",
        "SELECT MAX(tags) FROM grouped_tags",
    ];

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE grouped_tags (id TEXT, tags TEXT[])",
                vec![],
            )
            .expect("create table");
        for id in ["a", "b"] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO grouped_tags (id, tags) VALUES ($1, $2)",
                    vec![
                        Value::String(id.to_string()),
                        Value::Json(serde_json::json!(["x"])),
                    ],
                )
                .expect("insert row");
        }

        // Act
        let mut first_rows = Vec::new();
        for sql in queries {
            let (frames, server) = start_extended_query(
                cassie.clone(),
                support::parse_frame("grouped_array_stmt", sql),
                support::bind_frame_with_formats(
                    "grouped_array_portal",
                    "grouped_array_stmt",
                    &[],
                    &[],
                    &[1],
                ),
                support::execute_frame("grouped_array_portal"),
            )
            .await;
            server.stop().await;
            first_rows.push(
                frames
                    .iter()
                    .find(|frame| frame.0 == b'D')
                    .map(|frame| read_binary_row(&frame.1)),
            );
        }

        // Assert
        assert!(first_rows[0].is_some());
        assert_eq!(first_rows, vec![first_rows[0].clone(); queries.len()]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_encode_bigint_sum_as_exact_binary_numeric() {
    // Arrange
    const EXACT_SUM: i64 = 9_007_199_254_740_993;
    let path = support::data_dir("binary-integer-sum");

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
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
        let descriptor = (
            fields[0].type_oid,
            fields[0].type_size,
            fields[0].type_mod,
            fields[0].format_code,
        );
        let row = frames
            .iter()
            .find(|frame| frame.0 == b'D')
            .expect("data row");
        let actual_data = read_binary_row(&row.1);
        let expected_data = vec![Some(
            [
                0x00, 0x04, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x23, 0x2f, 0x07, 0xc8, 0x15, 0x62,
                0x03, 0xe1,
            ]
            .to_vec(),
        )];
        assert_eq!(
            (descriptor, actual_data),
            ((1700, -1, -1, 1), expected_data),
            "SUM(bigint) descriptor and binary DataRow"
        );

        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}
