#[test]
fn should_cancel_tablesample_filter_without_partial_rows_or_stale_state() {
    // Arrange
    let path = wire::data_dir("issue-800-tablesample-cancel");
    let _directory = GrammarFixtureDirectory(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let (setup, baseline, cancelled, recovery) = runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
        cassie
            .startup()
            .expect("start TABLESAMPLE cancellation fixture");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (mut reader, mut writer) = socket.into_split();
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_sample_cancel_rows (id INT); \
                 INSERT INTO issue_800_sample_cancel_rows VALUES (1), (2), (3)",
            )],
        )
        .await;
        let setup = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_cancel_rows \
                 TABLESAMPLE BERNOULLI (100) \
                 WHERE pg_sleep_for(INTERVAL '0 seconds') IS NOT NULL ORDER BY id",
            )],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_cancel_rows \
                 TABLESAMPLE BERNOULLI (100) \
                 WHERE pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL ORDER BY id",
            )],
        )
        .await;
        tokio::time::sleep(Duration::from_millis(40)).await;
        let mut cancel_socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write cancel request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancel request");
        let cancelled =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 45")]).await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.stop().await;
        (setup, baseline, cancelled, recovery)
    });

    // Assert
    assert_eq!(wire::error_code(&setup), None);
    assert_eq!(wire::error_code(&baseline), None);
    assert_eq!(
        wire::data_rows(&baseline),
        vec![
            vec![Some("1".to_owned())],
            vec![Some("2".to_owned())],
            vec![Some("3".to_owned())],
        ]
    );
    assert_eq!(wire::error_code(&cancelled).as_deref(), Some("57014"));
    assert_eq!(
        wire::data_rows(&cancelled),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(cancelled.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    assert_eq!(cancelled.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("45".to_owned())]]
    );
}
