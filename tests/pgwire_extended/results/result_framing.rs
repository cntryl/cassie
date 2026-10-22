use std::time::Duration;

use cassie::app::Cassie;

use super::support_pgwire as support;

const ANSWER_LIMIT: Duration = Duration::from_secs(10);
const OID_BOOL: i32 = 16;

type Frames = Vec<(u8, Vec<u8>)>;

fn configured_cassie(label: &str, setup: &[&str]) -> (Cassie, String) {
    let path = support::data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in setup {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("setup sql");
    }
    (cassie, path)
}

/// Runs one Parse/Bind/Describe/Execute/Sync cycle requesting binary
/// results and returns every frame through `ReadyForQuery`.
fn binary_round_trip(cassie: Cassie, path: String, sql: &str) -> Frames {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let frames = runtime.block_on(async {
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut writer).await;
        support::write_frames(
            &mut writer,
            vec![
                support::parse_frame("", sql),
                support::bind_frame_with_formats("", "", &[], &[], &[1]),
                support::describe_portal_frame(""),
                support::execute_frame(""),
                support::sync_frame(),
            ],
        )
        .await;
        let frames = support::read_frames_until_ready_within(&mut reader, ANSWER_LIMIT).await;
        server.stop().await;
        frames
    });
    let _ = std::fs::remove_dir_all(path);
    frames
}

/// Runs `sql` over the simple-query protocol and returns every frame
/// through `ReadyForQuery`.
fn simple_round_trip(cassie: Cassie, path: String, sql: &str) -> Frames {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let frames = runtime.block_on(async {
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut writer).await;
        support::write_frames(&mut writer, vec![support::simple_query_frame(sql)]).await;
        let frames = support::read_frames_until_ready_within(&mut reader, ANSWER_LIMIT).await;
        server.stop().await;
        frames
    });
    let _ = std::fs::remove_dir_all(path);
    frames
}

/// Runs one Parse/Describe(statement)/Bind/Execute/Sync cycle in text
/// format and returns every frame through `ReadyForQuery`.
fn extended_text_round_trip(cassie: Cassie, path: String, sql: &str) -> Frames {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let frames = runtime.block_on(async {
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut writer).await;
        support::write_frames(
            &mut writer,
            vec![
                support::parse_frame("", sql),
                support::describe_statement_frame(""),
                support::bind_frame("", "", &[]),
                support::execute_frame(""),
                support::sync_frame(),
            ],
        )
        .await;
        let frames = support::read_frames_until_ready_within(&mut reader, ANSWER_LIMIT).await;
        server.stop().await;
        frames
    });
    let _ = std::fs::remove_dir_all(path);
    frames
}

/// Asserts at least one `DataRow` arrived and every `DataRow` carries
/// exactly as many fields as the `RowDescription` declared.
fn assert_rows_match_description(frames: &Frames) {
    let described = support::row_description_names(frames).len();
    let rows = support::data_rows(frames);
    assert!(!rows.is_empty(), "expected at least one DataRow");
    for row in rows {
        assert_eq!(
            row.len(),
            described,
            "DataRow width must match RowDescription"
        );
    }
}

#[test]
fn should_describe_every_joined_column_given_wildcard_over_left_join() {
    // Arrange
    let (cassie, path) = configured_cassie(
        "wildcard-left-join",
        &[
            "CREATE TABLE wj_l (k INT, name TEXT)",
            "CREATE TABLE wj_r (k INT, total INT)",
            "INSERT INTO wj_l (k, name) VALUES (1, 'a'), (2, 'b')",
            "INSERT INTO wj_r (k, total) VALUES (1, 10)",
        ],
    );

    // Act
    let frames = simple_round_trip(
        cassie,
        path,
        "SELECT * FROM wj_l LEFT JOIN wj_r ON wj_l.k = wj_r.k",
    );

    // Assert
    assert_eq!(
        support::row_description_names(&frames),
        vec!["id", "k", "name", "id", "k", "total"]
    );
    assert_rows_match_description(&frames);
}

#[test]
fn should_describe_view_columns_without_phantom_id_given_wildcard_over_view() {
    // Arrange
    let (cassie, path) = configured_cassie(
        "wildcard-view",
        &[
            "CREATE TABLE wv_t (i INT, k TEXT)",
            "INSERT INTO wv_t (i, k) VALUES (1, 'a')",
            "CREATE VIEW wv_v AS SELECT i, k FROM wv_t",
        ],
    );

    // Act
    let frames = simple_round_trip(cassie, path, "SELECT * FROM wv_v");

    // Assert
    assert_eq!(support::row_description_names(&frames), vec!["i", "k"]);
    assert_eq!(
        support::data_rows(&frames),
        vec![vec![Some("1".to_string()), Some("a".to_string())]]
    );
}

#[test]
fn should_describe_derived_table_columns_given_wildcard_over_subquery() {
    // Arrange
    let (cassie, path) = configured_cassie(
        "wildcard-derived",
        &[
            "CREATE TABLE wd_t (name TEXT, n INT)",
            "INSERT INTO wd_t (name, n) VALUES ('alpha', 7)",
        ],
    );

    // Act
    let frames =
        extended_text_round_trip(cassie, path, "SELECT * FROM (SELECT name, n FROM wd_t) d");

    // Assert
    let descriptions = frames
        .iter()
        .filter(|frame| frame.0 == b'T')
        .map(|frame| support::parse_row_description(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 1);
    let names = descriptions[0]
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["name", "n"]);
    assert_eq!(
        support::data_rows(&frames),
        vec![vec![Some("alpha".to_string()), Some("7".to_string())]]
    );
}

#[test]
fn should_describe_table_function_columns_given_wildcard_over_table_function() {
    // Arrange
    let (cassie, path) = configured_cassie("wildcard-table-function", &[]);

    // Act
    let frames = simple_round_trip(cassie, path, "SELECT * FROM pg_show_all_settings()");

    // Assert
    let names = support::row_description_names(&frames);
    assert_eq!(names.len(), 17);
    assert_eq!(names.first().map(String::as_str), Some("name"));
    assert_rows_match_description(&frames);
}

#[test]
fn should_describe_cte_columns_without_phantom_id_given_wildcard_over_cte() {
    // Arrange
    let (cassie, path) = configured_cassie(
        "wildcard-cte",
        &[
            "CREATE TABLE wc_t (name TEXT, n INT)",
            "INSERT INTO wc_t (name, n) VALUES ('alpha', 7)",
        ],
    );

    // Act
    let frames = simple_round_trip(
        cassie,
        path,
        "WITH c AS (SELECT name, n FROM wc_t) SELECT * FROM c",
    );

    // Assert
    assert_eq!(support::row_description_names(&frames), vec!["name", "n"]);
    assert_eq!(
        support::data_rows(&frames),
        vec![vec![Some("alpha".to_string()), Some("7".to_string())]]
    );
}

fn data_row_field_lengths(payload: &[u8]) -> Vec<i32> {
    let count = i16::from_be_bytes([payload[0], payload[1]]);
    let mut cursor = 2_usize;
    let mut lengths = Vec::new();
    for _ in 0..count {
        let length = i32::from_be_bytes(
            payload[cursor..cursor + 4]
                .try_into()
                .expect("field length"),
        );
        cursor += 4 + usize::try_from(length.max(0)).expect("non-negative length");
        lengths.push(length);
    }
    assert_eq!(cursor, payload.len(), "DataRow body must be fully consumed");
    lengths
}

#[test]
fn should_answer_with_error_given_binary_row_wider_than_row_description() {
    // Arrange
    let (cassie, path) = configured_cassie(
        "binary-set-operation-width",
        &[
            "CREATE TABLE framing_left (a INT)",
            "CREATE TABLE framing_right (a INT, b INT)",
            "INSERT INTO framing_right (a, b) VALUES (1, 2)",
        ],
    );

    // Act
    let frames = binary_round_trip(
        cassie,
        path,
        "SELECT a FROM framing_left UNION SELECT a, b FROM framing_right",
    );

    // Assert
    let description = frames
        .iter()
        .find(|frame| frame.0 == b'T')
        .map(|frame| support::parse_row_description(&frame.1));
    for (_, payload) in frames.iter().filter(|frame| frame.0 == b'D') {
        let width = data_row_field_lengths(payload).len();
        assert_eq!(Some(width), description.as_ref().map(Vec::len));
    }
    assert!(frames.iter().any(|frame| frame.0 == b'E'));
    assert!(!frames.iter().any(|frame| frame.0 == b'D'));
    assert!(!frames.iter().any(|frame| frame.0 == b'C'));
    assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
}

#[test]
fn should_encode_json_scalars_given_binary_result_format() {
    // Arrange
    let (cassie, path) = configured_cassie(
            "binary-json-scalars",
            &[
                "CREATE TABLE framing_json (id TEXT, doc JSON)",
                "INSERT INTO framing_json (id, doc) VALUES ('a-arr', '[1,2]'), ('b-bool', 'true'), ('c-num', '7'), ('d-float', '2.5'), ('e-obj', '{\"a\":1}')",
            ],
        );

    // Act
    let frames = binary_round_trip(cassie, path, "SELECT doc FROM framing_json ORDER BY id");

    // Assert
    assert_eq!(support::error_code(&frames), None);
    assert_eq!(
        support::data_rows(&frames),
        vec![
            vec![Some("[1,2]".to_string())],
            vec![Some("true".to_string())],
            vec![Some("7".to_string())],
            vec![Some("2.5".to_string())],
            vec![Some("{\"a\":1}".to_string())],
        ]
    );
}

#[test]
fn should_render_bool_as_postgres_text_given_text_result_format() {
    // Arrange
    let (cassie, path) = configured_cassie(
        "text-bool-output",
        &[
            "CREATE TABLE framing_text_flags (id INT NOT NULL, flag BOOLEAN)",
            "INSERT INTO framing_text_flags (id, flag) VALUES (1, true), (2, false), (3, NULL)",
        ],
    );

    // Act
    let frames = simple_round_trip(
        cassie,
        path,
        "SELECT flag, id > 1 FROM framing_text_flags ORDER BY id",
    );

    // Assert
    assert_eq!(
        support::data_rows(&frames),
        vec![
            vec![Some("t".to_string()), Some("f".to_string())],
            vec![Some("f".to_string()), Some("t".to_string())],
            vec![None, Some("t".to_string())],
        ]
    );
}

#[test]
fn should_render_infinite_float8_as_postgres_text_given_text_result_format() {
    // Arrange
    let (cassie, path) = configured_cassie("text-float8-infinity", &[]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let positive = f64::INFINITY.to_be_bytes();
    let negative = f64::NEG_INFINITY.to_be_bytes();

    // Act
    let frames = runtime.block_on(async {
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut writer).await;
        support::write_frames(
            &mut writer,
            vec![
                support::parse_frame_with_types("", "SELECT $1 AS v, $2 AS w", &[701, 701]),
                support::bind_frame_with_formats(
                    "",
                    "",
                    &[1],
                    &[Some(&positive), Some(&negative)],
                    &[0],
                ),
                support::execute_frame(""),
                support::sync_frame(),
            ],
        )
        .await;
        let frames = support::read_frames_until_ready_within(&mut reader, ANSWER_LIMIT).await;
        server.stop().await;
        frames
    });
    let _ = std::fs::remove_dir_all(path);

    // Assert
    assert_eq!(
        support::data_rows(&frames),
        vec![vec![
            Some("Infinity".to_string()),
            Some("-Infinity".to_string()),
        ]]
    );
}

#[test]
fn should_encode_catalog_bool_columns_as_declared_binary_bool() {
    // Arrange
    let (cassie, path) = configured_cassie(
        "catalog-binary-bool",
        &[
            "CREATE TABLE framing_flags (id INT NOT NULL, flag BOOLEAN)",
            "CREATE INDEX framing_flags_id ON framing_flags (id)",
        ],
    );

    // Act
    let frames = binary_round_trip(
        cassie,
        path,
        "SELECT indisunique, indisprimary FROM pg_catalog.pg_index",
    );

    // Assert
    let description = frames
        .iter()
        .find(|frame| frame.0 == b'T')
        .map(|frame| support::parse_row_description(&frame.1))
        .expect("RowDescription");
    assert!(description
        .iter()
        .all(|field| field.type_oid == OID_BOOL && field.format_code == 1));
    let rows = frames
        .iter()
        .filter(|frame| frame.0 == b'D')
        .map(|frame| data_row_field_lengths(&frame.1))
        .collect::<Vec<_>>();
    assert_ne!(rows, [] as [std::vec::Vec<i32>; 0]);
    assert!(rows.iter().flatten().all(|length| *length == 1));
}
#[test]
fn should_keep_json_string_quotes_given_json_column_read() {
    // Arrange
    let setup = [
        "CREATE TABLE framing_json_text (id TEXT, doc JSON)",
        "INSERT INTO framing_json_text (id, doc) VALUES ('a', '\"hi\"'), ('b', '[1]')",
    ];
    let queries = [
        "SELECT doc FROM framing_json_text ORDER BY id",
        "SELECT doc FROM framing_json_text WHERE id = 'a'",
        "SELECT id, doc FROM framing_json_text",
        "SELECT DISTINCT doc FROM framing_json_text",
        "INSERT INTO framing_json_text (id, doc) VALUES ('c', '\"hi\"') RETURNING doc",
    ];

    // Act
    let rendered = queries
        .iter()
        .map(|sql| {
            let (cassie, path) = configured_cassie("text-json-string", &setup);
            let frames = simple_round_trip(cassie, path, sql);
            support::data_rows(&frames)
                .into_iter()
                .filter_map(|row| row.last().cloned().flatten())
                .filter(|doc| doc.contains("hi"))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(rendered, vec![vec!["\"hi\"".to_string()]; queries.len()]);
}
