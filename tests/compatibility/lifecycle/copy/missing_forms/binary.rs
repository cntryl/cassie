#[test]
fn should_frame_binary_query_copy_scalar_null_fields() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_binary_query_to_wave4").await;
        let (client, connection) = server.connect().await;

        // Act
        let frames = copy_to_stdout(
            &server,
            "COPY (SELECT 42::INT4 AS n, NULL::TEXT AS absent, 'ok'::TEXT AS payload) \
             TO STDOUT WITH (FORMAT BINARY)",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let response = frames
            .iter()
            .find(|(tag, _)| *tag == b'H')
            .map(|(_, payload)| payload.clone());
        let payload = copy_payload(&frames);
        let command = copy_command(&frames);
        let completed = frames.iter().any(|(tag, _)| *tag == b'c');

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            error_code, None,
            "binary query COPY TO SQLSTATE; frames: {frames:?}"
        );
        assert_eq!(response, Some(vec![1, 0, 3, 0, 1, 0, 1, 0, 1]));
        let mut expected = b"PGCOPY\n\xff\r\n\0".to_vec();
        expected.extend_from_slice(&0_i32.to_be_bytes()); // flags
        expected.extend_from_slice(&0_i32.to_be_bytes()); // extension length
        expected.extend_from_slice(&3_i16.to_be_bytes()); // field count
        expected.extend_from_slice(&4_i32.to_be_bytes()); // int4 field length
        expected.extend_from_slice(&42_i32.to_be_bytes());
        expected.extend_from_slice(&(-1_i32).to_be_bytes()); // NULL field length
        expected.extend_from_slice(&2_i32.to_be_bytes()); // text field length
        expected.extend_from_slice(b"ok");
        expected.extend_from_slice(&(-1_i16).to_be_bytes()); // end of rows
        assert_eq!(payload, expected);
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert!(completed, "binary query COPY TO should send CopyDone");
    });
}
