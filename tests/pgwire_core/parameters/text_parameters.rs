use cassie::app::Cassie;

use super::support_pgwire as support;

type WireFrame = (u8, Vec<u8>);
type SingleParameterQuery<'a> = (&'a str, i32, i16, &'a [u8]);

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn seed(cassie: &Cassie) {
    let session = cassie.create_session("tester", None);
    for sql in [
            "CREATE TABLE text_param_docs (k INT, ts TIMESTAMP, day DATE, clock TIME, tag UUID, blob BYTEA, score FLOAT, flag BOOLEAN)",
            "INSERT INTO text_param_docs (k, ts, day, clock, tag, blob, score, flag) VALUES (1, '2024-01-01 12:00:00', '2024-01-01', '12:00:00', '6ba7b810-9dad-11d1-80b4-00c04fd430c8', '\\xdeadbeef', 1.5, true)",
            "INSERT INTO text_param_docs (k, ts, day, clock, tag, blob, score, flag) VALUES (2, '2024-01-02 12:00:00', '2024-01-02', '13:00:00', '00000000-0000-0000-0000-000000000002', '\\x00', 2.5, false)",
            "INSERT INTO text_param_docs (k, ts, day, clock, tag, blob, score, flag) VALUES (3, '2024-01-03 12:00:00', '2024-01-03', '14:00:00', '00000000-0000-0000-0000-000000000003', '\\x01', 3.5, false)",
        ] {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed text parameter table");
        }
}

/// Runs each query as its own extended-query batch on one connection and
/// returns every batch's frames.
async fn run_single_parameter_queries(
    cassie: Cassie,
    queries: &[SingleParameterQuery<'_>],
) -> (Vec<Vec<WireFrame>>, support::PgwireServer) {
    let server = support::spawn_server(cassie).await;
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect pgwire");
    let (mut reader, mut writer) = tokio::io::split(socket);
    support::complete_startup(&mut reader, &mut writer).await;
    let mut batches = Vec::new();
    for (sql, oid, format, value) in queries {
        support::write_frames(
            &mut writer,
            vec![
                support::parse_frame_with_types("", sql, &[*oid]),
                support::bind_frame_with_formats("", "", &[*format], &[Some(value)], &[0]),
                support::execute_frame(""),
                support::sync_frame(),
            ],
        )
        .await;
        batches.push(support::read_frames_until_ready(&mut reader).await);
    }
    (batches, server)
}

fn run_against_seeded(label: &str, queries: &[SingleParameterQuery<'_>]) -> Vec<Vec<WireFrame>> {
    support::use_local_storage();
    let path = support::data_dir(label);
    let batches = runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        seed(&cassie);
        let (batches, server) = run_single_parameter_queries(cassie, queries).await;
        server.stop().await;
        batches
    });
    let _ = std::fs::remove_dir_all(path);
    batches
}

fn first_column(frames: &[WireFrame]) -> Vec<Option<String>> {
    support::data_rows(frames)
        .into_iter()
        .map(|row| row.into_iter().next().flatten())
        .collect()
}

fn keys(values: &[&str]) -> Vec<Option<String>> {
    values
        .iter()
        .map(|value| Some((*value).to_string()))
        .collect()
}

#[test]
fn should_match_text_temporal_parameters_like_binary_ones() {
    // Arrange
    let binary_ts = ((8_766_i64 * 86_400 + 12 * 3_600) * 1_000_000).to_be_bytes();
    let ts = "SELECT k FROM text_param_docs WHERE ts = $1";
    let queries: [SingleParameterQuery<'_>; 6] = [
        (ts, 1114, 1, &binary_ts),
        (ts, 1114, 0, b"2024-01-01 12:00:00"),
        (ts, 1114, 0, b"2024-01-01T12:00:00Z"),
        (
            "SELECT k FROM text_param_docs WHERE ts < $1 ORDER BY k",
            1114,
            0,
            b"2024-01-02 12:00:00.5",
        ),
        (
            "SELECT k FROM text_param_docs WHERE day = $1",
            1082,
            0,
            b" 2024-01-02 ",
        ),
        (
            "SELECT k FROM text_param_docs WHERE clock = $1",
            1083,
            0,
            b"14:00:00.000000",
        ),
    ];

    // Act
    let batches = run_against_seeded("text-temporal-parameters", &queries);

    // Assert
    assert_eq!(first_column(&batches[0]), keys(&["1"]));
    assert_eq!(first_column(&batches[1]), keys(&["1"]));
    assert_eq!(first_column(&batches[2]), keys(&["1"]));
    assert_eq!(first_column(&batches[3]), keys(&["1", "2"]));
    assert_eq!(first_column(&batches[4]), keys(&["2"]));
    assert_eq!(first_column(&batches[5]), keys(&["3"]));
}

#[test]
fn should_reject_malformed_text_temporal_parameters() {
    // Arrange
    let queries: [SingleParameterQuery<'_>; 3] = [
        (
            "SELECT k FROM text_param_docs WHERE ts = $1",
            1114,
            0,
            b"garbage",
        ),
        (
            "SELECT k FROM text_param_docs WHERE day = $1",
            1082,
            0,
            b"2024-13-01",
        ),
        (
            "SELECT k FROM text_param_docs WHERE clock = $1",
            1083,
            0,
            b"25:00:00",
        ),
    ];

    // Act
    let batches = run_against_seeded("text-temporal-rejects", &queries);

    // Assert
    for frames in &batches {
        assert_eq!(support::error_code(frames).as_deref(), Some("08P01"));
        assert_eq!(
            support::data_rows(frames),
            [] as [std::vec::Vec<std::option::Option<std::string::String>>; 0]
        );
    }
}

#[test]
fn should_echo_stored_timestamp_from_insert_returning_text_parameter() {
    // Arrange
    let queries: [SingleParameterQuery<'_>; 2] = [
        (
            "INSERT INTO text_param_docs (k, ts) VALUES (9, $1) RETURNING ts",
            1114,
            0,
            b"2024-06-01 08:00:00",
        ),
        ("SELECT ts FROM text_param_docs WHERE k = $1", 23, 0, b"9"),
    ];

    // Act
    let batches = run_against_seeded("text-timestamp-returning", &queries);

    // Assert
    let stored = keys(&["2024-06-01 08:00:00"]);
    assert_eq!(first_column(&batches[0]), stored);
    assert_eq!(first_column(&batches[1]), stored);
}

#[test]
fn should_decode_float4_numeric_text_parameters_as_numbers() {
    // Arrange
    let queries: [SingleParameterQuery<'_>; 3] = [
        (
            "SELECT k FROM text_param_docs WHERE score > $1 ORDER BY k",
            700,
            0,
            b"2.0",
        ),
        (
            "SELECT k FROM text_param_docs WHERE k > $1 ORDER BY k",
            1700,
            0,
            b"2",
        ),
        (
            "SELECT k FROM text_param_docs WHERE score < $1 ORDER BY k",
            1700,
            0,
            b" 2.75 ",
        ),
    ];

    // Act
    let batches = run_against_seeded("text-float4-numeric", &queries);

    // Assert
    assert_eq!(first_column(&batches[0]), keys(&["2", "3"]));
    assert_eq!(first_column(&batches[1]), keys(&["3"]));
    assert_eq!(first_column(&batches[2]), keys(&["1", "2"]));
}

#[test]
fn should_canonicalize_text_uuid_bytea_parameters_like_binary_ones() {
    // Arrange
    let binary_uuid = [
        0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30,
        0xc8,
    ];
    let tag = "SELECT k FROM text_param_docs WHERE tag = $1";
    let blob = "SELECT k FROM text_param_docs WHERE blob = $1";
    let queries: [SingleParameterQuery<'_>; 5] = [
        (tag, 2950, 1, &binary_uuid),
        (tag, 2950, 0, b"6BA7B810-9DAD-11D1-80B4-00C04FD430C8"),
        (blob, 17, 0, b"\\xDEADBEEF"),
        (tag, 2950, 0, b"not-a-uuid"),
        (blob, 17, 0, b"\\xabc"),
    ];

    // Act
    let batches = run_against_seeded("text-uuid-bytea", &queries);

    // Assert
    assert_eq!(first_column(&batches[0]), keys(&["1"]));
    assert_eq!(first_column(&batches[1]), keys(&["1"]));
    assert_eq!(first_column(&batches[2]), keys(&["1"]));
    assert_eq!(support::error_code(&batches[3]).as_deref(), Some("08P01"));
    assert_eq!(support::error_code(&batches[4]).as_deref(), Some("08P01"));
}

#[test]
fn should_accept_postgres_text_bool_integer_spellings() {
    // Arrange
    let flag = "SELECT k FROM text_param_docs WHERE flag = $1 ORDER BY k";
    let key = "SELECT k FROM text_param_docs WHERE k = $1";
    let queries: [SingleParameterQuery<'_>; 9] = [
        (flag, 16, 0, b"yes"),
        (flag, 16, 0, b"ON"),
        (flag, 16, 0, b" true "),
        (flag, 16, 0, b"tr"),
        (flag, 16, 0, b"of"),
        (flag, 16, 0, b"o"),
        (key, 23, 0, b" 1"),
        (key, 23, 0, b"3 "),
        (key, 21, 0, b"40000"),
    ];

    // Act
    let batches = run_against_seeded("text-bool-integer-spellings", &queries);

    // Assert
    for frames in &batches[..4] {
        assert_eq!(first_column(frames), keys(&["1"]));
    }
    assert_eq!(first_column(&batches[4]), keys(&["2", "3"]));
    assert_eq!(support::error_code(&batches[5]).as_deref(), Some("08P01"));
    assert_eq!(first_column(&batches[6]), keys(&["1"]));
    assert_eq!(first_column(&batches[7]), keys(&["3"]));
    assert_eq!(support::error_code(&batches[8]).as_deref(), Some("22003"));
}
