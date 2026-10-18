use super::*;

#[test]
fn should_export_binary_int4_array_with_null_element_framing() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_binary_array_out").await;
        let (client, connection) = server.connect().await;

        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        support_pgwire::complete_startup(&mut reader, &mut writer).await;

        // Act
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY (SELECT ARRAY[1,NULL,3]::INT4[] AS value) \
                 TO STDOUT WITH (FORMAT BINARY)",
            )],
        )
        .await;
        let frames =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        let error_code = support_pgwire::error_code(&frames);
        let copy_out_response = frames
            .iter()
            .find(|(tag, _)| *tag == b'H')
            .map(|(_, payload)| payload.clone());
        let copy_data = frames
            .iter()
            .filter(|(tag, _)| *tag == b'd')
            .flat_map(|(_, payload)| payload.iter().copied())
            .collect::<Vec<_>>();
        let command = frames
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, payload)| {
                String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
            });
        let completed_copy_out = frames.iter().any(|(tag, _)| *tag == b'c');

        drop((reader, writer, client));
        server.shutdown(connection).await;

        // Assert
        assert_eq!(
            error_code, None,
            "binary COPY TO SQLSTATE; frames: {frames:?}"
        );
        let response_payload = copy_out_response.unwrap_or_else(|| {
            panic!(
                "binary COPY TO should send CopyOutResponse; frames: {frames:?}; \
                 SQLSTATE: {error_code:?}"
            )
        });
        assert_eq!(response_payload, [1, 0, 1, 0, 1]);
        assert!(completed_copy_out, "binary COPY TO should send CopyDone");
        assert_eq!(command.as_deref(), Some("COPY 1"));

        let mut expected = b"PGCOPY\n\xff\r\n\0".to_vec();
        expected.extend_from_slice(&0_i32.to_be_bytes()); // flags
        expected.extend_from_slice(&0_i32.to_be_bytes()); // extension length
        expected.extend_from_slice(&1_i16.to_be_bytes()); // field count
        expected.extend_from_slice(&40_i32.to_be_bytes()); // int4[] binary payload length
        expected.extend_from_slice(&1_i32.to_be_bytes()); // dimensions
        expected.extend_from_slice(&1_i32.to_be_bytes()); // has NULL elements
        expected.extend_from_slice(&23_i32.to_be_bytes()); // int4 element OID
        expected.extend_from_slice(&3_i32.to_be_bytes()); // dimension length
        expected.extend_from_slice(&1_i32.to_be_bytes()); // lower bound
        expected.extend_from_slice(&4_i32.to_be_bytes());
        expected.extend_from_slice(&1_i32.to_be_bytes());
        expected.extend_from_slice(&(-1_i32).to_be_bytes());
        expected.extend_from_slice(&4_i32.to_be_bytes());
        expected.extend_from_slice(&3_i32.to_be_bytes());
        expected.extend_from_slice(&(-1_i16).to_be_bytes()); // end of rows
        assert_eq!(copy_data, expected);
    });
}
