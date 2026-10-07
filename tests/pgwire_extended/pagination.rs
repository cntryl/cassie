use crate::support_pgwire as wire;
use cassie::app::Cassie;
use tokio::io::BufReader;

#[test]
fn should_preserve_wire_pagination_contract() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("pagination-wire");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE records (n BIGINT)",
            "INSERT INTO records VALUES (1), (2), (3), (4), (5)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect("setup");
        }
        let server = wire::spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut writer) = socket.split();
        let mut reader = BufReader::new(read_half);
        wire::complete_startup(&mut reader, &mut writer).await;
        // Act
        verify_cumulative_portal(&cassie, &session, &mut reader, &mut writer).await;
        verify_bound_errors(&mut reader, &mut writer).await;
        verify_binary_bounds(&mut reader, &mut writer).await;

        drop(socket);
        server.stop().await;
    });
    // Assert
    std::fs::remove_dir_all(path).expect("cleanup");
}

async fn verify_cumulative_portal(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
) {
    wire::transaction_control(reader, writer, "BEGIN").await;

    // Act
    wire::write_frames(
        writer,
        vec![
            wire::parse_frame(
                "bounds",
                "SELECT n FROM records ORDER BY n LIMIT $1 OFFSET $2",
            ),
            wire::describe_statement_frame("bounds"),
            wire::bind_frame("bounded", "bounds", &["3", "1"]),
            wire::execute_limited_frame("bounded", 2),
            wire::sync_frame(),
        ],
    )
    .await;
    let first = wire::read_frames_until_ready(reader).await;
    cassie
        .execute_sql(session, "INSERT INTO records VALUES (6)", vec![])
        .expect("concurrent insert");
    wire::write_frames(
        writer,
        vec![
            wire::execute_limited_frame("bounded", 2),
            wire::sync_frame(),
        ],
    )
    .await;
    let second = wire::read_frames_until_ready(reader).await;

    // Assert
    assert_eq!(wire::error_code(&first), None);
    let parameters = first
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("parameter descriptor");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![20, 20]
    );
    assert!(
        first.iter().any(|frame| frame.0 == b's'),
        "first page suspends"
    );
    assert_eq!(
        wire::data_rows(&first),
        vec![vec![Some("2".into())], vec![Some("3".into())]]
    );
    assert_eq!(wire::error_code(&second), None);
    assert_eq!(wire::data_rows(&second), vec![vec![Some("4".into())]]);
    assert!(
        second.iter().any(|frame| frame.0 == b'C'),
        "cumulative LIMIT completes"
    );
    wire::write_frames(writer, vec![wire::simple_query_frame("COMMIT")]).await;
    let committed = wire::read_frames_until_ready(reader).await;
    assert_eq!(wire::error_code(&committed), None);
    assert_eq!(
        committed.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
}

async fn verify_bound_errors(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
) {
    // Arrange
    let cases = [
        ("SELECT n FROM records LIMIT $1", vec!["-1"], "2201W"),
        ("SELECT n FROM records OFFSET $1", vec!["-1"], "2201X"),
        (
            "SELECT n FROM records LIMIT CAST($1 AS BIGINT) + 1",
            vec!["9223372036854775807"],
            "22003",
        ),
        (
            "SELECT n FROM records LIMIT $1 + $2",
            vec!["1", "2"],
            "42725",
        ),
    ];
    for (index, (sql, params, expected)) in cases.into_iter().enumerate() {
        let statement = format!("error_case_{index}");
        let portal = format!("error_portal_{index}");
        // Act
        wire::write_frames(
            writer,
            vec![
                wire::parse_frame(&statement, sql),
                wire::bind_frame(&portal, &statement, &params),
                wire::execute_frame(&portal),
                wire::sync_frame(),
            ],
        )
        .await;
        let frames = wire::read_frames_until_ready(reader).await;

        // Assert
        assert_eq!(
            wire::error_code(&frames).as_deref(),
            Some(expected),
            "{sql}"
        );
    }
}

async fn verify_binary_bounds(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
) {
    // Arrange
    let limit = 2_i64.to_be_bytes();
    let offset = 1_i64.to_be_bytes();

    // Act
    wire::write_frames(
        writer,
        vec![
            wire::parse_frame(
                "binary_bounds",
                "SELECT n FROM records ORDER BY n LIMIT $1 OFFSET $2",
            ),
            wire::bind_frame_with_formats(
                "binary_portal",
                "binary_bounds",
                &[1],
                &[Some(&limit), Some(&offset)],
                &[1],
            ),
            wire::execute_frame("binary_portal"),
            wire::sync_frame(),
        ],
    )
    .await;
    let binary = wire::read_frames_until_ready(reader).await;

    // Assert
    assert_eq!(wire::error_code(&binary), None);
    let values: Vec<_> = binary
        .iter()
        .filter(|frame| frame.0 == b'D')
        .map(|(_, payload)| {
            assert_eq!(&payload[..6], &[0, 1, 0, 0, 0, 8]);
            i64::from_be_bytes(payload[6..14].try_into().expect("BIGINT binary result"))
        })
        .collect();
    assert_eq!(values, vec![2, 3]);
}
