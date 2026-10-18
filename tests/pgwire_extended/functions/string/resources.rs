#[test]
fn should_resume_generate_series_portal_after_each_execute_row_limit() {
    // Arrange
    let (storage, config) = configured_fixture("issue-808-srf-portal-row-limit", 2);
    let runtime = current_thread_runtime();

    // Act
    let frames = runtime.block_on(async {
        let (address, server) = start_server(storage.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![
                wire::parse_frame(
                    "bounded_series_statement",
                    "SELECT value FROM generate_series(1,2) AS series(value)",
                ),
                wire::describe_statement_frame("bounded_series_statement"),
                wire::bind_frame("bounded_series_portal", "bounded_series_statement", &[]),
                wire::describe_portal_frame("bounded_series_portal"),
                wire::execute_limited_frame("bounded_series_portal", 1),
                wire::execute_limited_frame("bounded_series_portal", 1),
                wire::sync_frame(),
            ],
        )
        .await;
        let frames =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        frames
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    let row_description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("generate_series portal RowDescription");
    assert_eq!(
        wire::parse_row_description(&row_description.1)[0].type_oid,
        23
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]]
    );
    let suspended = frames
        .iter()
        .position(|(tag, _)| *tag == b's')
        .expect("first Execute suspends after one row");
    assert_eq!(
        frames[..suspended]
            .iter()
            .filter(|(tag, _)| *tag == b'D')
            .count(),
        1
    );
    assert_eq!(
        frames[suspended + 1..]
            .iter()
            .filter(|(tag, _)| *tag == b'D')
            .count(),
        1
    );
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 1);
    assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
}
