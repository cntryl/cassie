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
