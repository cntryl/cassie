#[test]
fn should_abort_copy_from_stdin_atomically_on_malformed_input() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_malformed_atomicity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_malformed_atomicity (id INT)")
            .await
            .expect("create table for malformed COPY input");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_malformed_atomicity FROM STDIN WITH (FORMAT CSV)",
            b"1\nbad\n3\n",
        )
        .await;
        let rows = client
            .query(
                "SELECT id FROM compat_copy_malformed_atomicity ORDER BY id",
                &[],
            )
            .await
            .expect("read target rows after malformed COPY fails");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "valid COPY FROM STDIN should enter COPY IN; payload: {response_payload:?}"
        );
        assert_eq!(error_code.as_deref(), Some("22P02"));
        assert!(
            command.is_none(),
            "failed COPY must not send CommandComplete"
        );
        assert!(
            rows.is_empty(),
            "COPY must roll back rows accepted before the malformed input row"
        );
    });
}

#[test]
fn should_preserve_copy_input_error_contract_given_selected_scalar_types() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_scalar_input_diagnostics").await;
        let (client, connection) = server.connect().await;
        let cases = [
            ("SMALLINT", "32768", "22003"),
            ("INT", "bad", "22P02"),
            ("INT", "2147483648", "22003"),
            ("BIGINT", "9223372036854775808", "22003"),
            ("DOUBLE PRECISION", "bad", "22P02"),
            ("DOUBLE PRECISION", "1e400", "22003"),
            ("BOOLEAN", "perhaps", "22P02"),
            ("NUMERIC", "bad", "22P02"),
            ("JSON", "bad", "22P02"),
            ("JSONB", "bad", "22P02"),
            ("OID", "bad", "22P02"),
        ];

        // Act
        let mut observations = Vec::new();
        for (data_type, rejected, expected) in cases {
            client.batch_execute("DROP TABLE IF EXISTS copy_input_probe")
                .await.expect("remove prior scalar COPY target");
            client.batch_execute(&format!("CREATE TABLE copy_input_probe (v {data_type})"))
                .await.expect("create scalar COPY target");
            let payload = format!("1\n{rejected}\n");
            let frames = copy_from_stdin_with_frames(&server,
                "COPY copy_input_probe FROM STDIN WITH (FORMAT CSV)", payload.as_bytes()).await;
            let count: i64 = client.query_one("SELECT count(*) FROM copy_input_probe", &[])
                .await.expect("inspect target after COPY error").get(0);
            observations.push((data_type, expected, support_pgwire::error_code(&frames), count,
                frames.iter().any(|(tag, _)| *tag == b'C'), frames.last().cloned()));
        }
        drop(client);
        server.shutdown(connection).await;

        // Assert
        for (data_type, expected, actual, count, completed, last) in observations {
            assert_eq!(actual.as_deref(), Some(expected), "{data_type}");
            assert_eq!(count, 0, "{data_type} must not publish the earlier valid row");
            assert!(!completed, "{data_type} must not report successful COPY completion");
            assert_eq!(last, Some((b'Z', vec![b'I'])), "{data_type} must restore readiness");
        }
    });
}

#[test]
fn should_report_copy_csv_structure_errors_without_publishing_earlier_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_csv_structure_diagnostics").await;
        let (client, connection) = server.connect().await;
        client.batch_execute("CREATE TABLE copy_structure_probe (v TEXT)")
            .await.expect("create COPY target");

        // Act
        let mut observations = Vec::new();
        for payload in [b"valid\n\"unterminated".as_slice(), b"valid\nextra,column\n"] {
            let frames = copy_from_stdin_with_frames(&server,
                "COPY copy_structure_probe FROM STDIN WITH (FORMAT CSV)", payload).await;
            let count: i64 = client.query_one("SELECT count(*) FROM copy_structure_probe", &[])
                .await.expect("inspect failed COPY target").get(0);
            observations.push((support_pgwire::error_code(&frames), count,
                frames.iter().any(|(tag, _)| *tag == b'C')));
        }
        drop(client);
        server.shutdown(connection).await;

        // Assert
        for (state, count, completed) in observations {
            assert_eq!(state.as_deref(), Some("22P04"));
            assert_eq!(count, 0);
            assert!(!completed);
        }
    });
}
