use super::*;

#[test]
fn should_decode_default_text_copy_newline_escape() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_text_newline_escape").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_text_newline_escape (payload TEXT)")
            .await
            .expect("create table for default-text COPY newline escape");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_text_newline_escape FROM STDIN",
            b"line\\nbreak\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query("SELECT payload FROM compat_copy_text_newline_escape", &[])
                .await
                .expect("read value imported by default-text COPY")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "default-text COPY should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].get::<_, Option<String>>(0),
            Some("line\nbreak".to_string())
        );
    });
}

#[test]
fn should_preserve_escaped_default_text_null_marker_as_text() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_text_null_escape").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_text_null_escape (payload TEXT)")
            .await
            .expect("create table for default-text COPY null marker");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_text_null_escape FROM STDIN",
            b"\\\\N\n\\N\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query("SELECT payload FROM compat_copy_text_null_escape", &[])
                .await
                .expect("read values imported by default-text COPY")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "default-text COPY should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(rows.len(), 2);
        let mut values = rows
            .iter()
            .map(|row| row.get::<_, Option<String>>(0))
            .collect::<Vec<_>>();
        values.sort();
        assert_eq!(values, vec![None, Some("\\N".to_string())]);
    });
}

#[test]
fn should_decode_default_text_copy_escaped_tab() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_text_escaped_tab").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_text_tab (payload TEXT)")
            .await
            .expect("create table for a default-text COPY tab escape");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_text_tab FROM STDIN",
            b"line\\tdetail\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query("SELECT payload FROM compat_copy_text_tab", &[])
                .await
                .expect("read values imported by default-text COPY")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "default-text COPY should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, String>(0), "line\tdetail");
    });
}

#[test]
fn should_import_standalone_backslash_dot_as_a_csv_field() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_csv_backslash_dot").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_csv_backslash_dot (payload TEXT)")
            .await
            .expect("create table for a standalone CSV backslash-dot field");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_csv_backslash_dot FROM STDIN WITH (FORMAT csv)",
            b"before\n\\.\nafter\n",
        )
        .await;
        let mut values = if response_tag == b'G' {
            client
                .query("SELECT payload FROM compat_copy_csv_backslash_dot", &[])
                .await
                .expect("read CSV values after COPY")
                .iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        values.sort();

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "CSV COPY should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 3"));
        assert_eq!(
            values,
            vec!["\\.".to_string(), "after".to_string(), "before".to_string()]
        );
    });
}

#[test]
fn should_import_escaped_tab_row_before_end_marker() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_text_escaped_delimiter_eof").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_text_eof (a TEXT, b TEXT)")
            .await
            .expect("create table for escaped-delimiter COPY");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_text_eof FROM STDIN",
            b"x\\\ty\tz\n\\.\nignored\trow\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query("SELECT a, b FROM compat_copy_text_eof", &[])
                .await
                .expect("read values imported by text COPY")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "text COPY should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, String>(0), "x\ty");
        assert_eq!(rows[0].get::<_, String>(1), "z");
    });
}

#[test]
fn should_reject_mixed_text_copy_record_endings_before_applying_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_text_record_endings").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE copy_record_endings (payload TEXT)")
            .await
            .expect("create text COPY target");
        let cases: [(&[u8], Option<&str>, &[&str]); 4] = [
            (b"a\nb\r\n", Some("22P04"), &[]),
            (b"a\r\nb\n", Some("22P04"), &[]),
            (b"a\nb\n", None, &["a", "b"]),
            (b"a\r\nb\r\n", None, &["a", "b"]),
        ];

        for (payload, expected_error, expected_values) in cases {
            // Act
            let (response_tag, _, error, command) =
                copy_from_stdin(&server, "COPY copy_record_endings FROM STDIN", payload).await;
            let values: Vec<String> = client
                .query(
                    "SELECT payload FROM copy_record_endings ORDER BY payload",
                    &[],
                )
                .await
                .expect("read persisted text COPY rows")
                .iter()
                .map(|row| row.get(0))
                .collect();

            // Assert
            assert_eq!(response_tag, b'G', "{payload:?}");
            assert_eq!(error.as_deref(), expected_error, "{payload:?}");
            assert_eq!(
                command.as_deref(),
                expected_error.is_none().then_some("COPY 2")
            );
            assert_eq!(values, expected_values, "{payload:?}");
            client
                .batch_execute("DELETE FROM copy_record_endings")
                .await
                .expect("clear target for the next case");
        }
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_validate_text_copy_encoding_only_through_the_end_marker() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_text_encoding_marker").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE copy_encoding_marker (payload TEXT)")
            .await
            .expect("create text COPY target");
        let cases: [(&[u8], Option<&str>, &[&str]); 3] = [
            (b"good\n\xff\n\\.\n", Some("22021"), &[]),
            (b"good\n\\.\n\xff\n", None, &["good"]),
            (b"good\n\\.\nignored\tfield\n", None, &["good"]),
        ];

        let statements = [
            "COPY copy_encoding_marker FROM STDIN",
            "COPY copy_encoding_marker FROM STDIN WITH (FORMAT TEXT, ENCODING 'UTF8')",
        ];

        for (sql, (payload, expected_error, expected_values)) in statements
            .into_iter()
            .flat_map(|sql| cases.into_iter().map(move |case| (sql, case)))
        {
            // Act
            let (response_tag, _, error, command) = copy_from_stdin(&server, sql, payload).await;
            let values: Vec<String> = client
                .query(
                    "SELECT payload FROM copy_encoding_marker ORDER BY payload",
                    &[],
                )
                .await
                .expect("read persisted text COPY rows")
                .iter()
                .map(|row| row.get(0))
                .collect();

            // Assert
            assert_eq!(response_tag, b'G', "{sql}: {payload:?}");
            assert_eq!(error.as_deref(), expected_error, "{sql}: {payload:?}");
            assert_eq!(
                command.as_deref(),
                expected_error.is_none().then_some("COPY 1"),
                "{sql}: {payload:?}"
            );
            assert_eq!(values, expected_values, "{sql}: {payload:?}");
            client
                .batch_execute("DELETE FROM copy_encoding_marker")
                .await
                .expect("clear target for the next case");
        }
        drop(client);
        server.shutdown(connection).await;
    });
}
