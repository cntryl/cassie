#[test]
fn should_cancel_rows_from_set_expansion_without_partial_rows_or_stale_cancellation() {
    // Arrange
    let (storage, config) = configured_fixture("issue-808-srf-cancel", 3);
    let runtime = current_thread_runtime();

    // Act
    let (baseline, cancelled, recovery) = runtime.block_on(async {
        let (address, server) = start_server(storage.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("query connection");
        let (mut reader, mut writer) = tokio::io::split(socket);
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

        let baseline_query = "SELECT array_value, series_value FROM ROWS FROM (unnest(ARRAY[1,2,3]), generate_series(1,3)) AS expanded(array_value, series_value) WHERE series_value < 3 OR pg_sleep_for(INTERVAL '0 seconds') IS NOT NULL";
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(baseline_query)],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        let cancellable_query = "SELECT array_value, series_value FROM ROWS FROM (unnest(ARRAY[1,2,3]), generate_series(1,3)) AS expanded(array_value, series_value) WHERE series_value < 3 OR pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL";
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(cancellable_query)],
        )
        .await;
        tokio::time::sleep(Duration::from_millis(40)).await;
        let mut cancel_socket = tokio::net::TcpStream::connect(address)
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
        server.abort();
        let _ = server.await;
        (baseline, cancelled, recovery)
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert_eq!(wire::error_code(&baseline), None);
    assert_eq!(
        wire::data_rows(&baseline),
        vec![
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), Some("2".to_owned())],
            vec![Some("3".to_owned()), Some("3".to_owned())],
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
