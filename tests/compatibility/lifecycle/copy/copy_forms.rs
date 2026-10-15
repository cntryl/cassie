use super::*;
use std::time::Duration;

#[path = "binary_array.rs"]
mod binary_array;
#[path = "binary_copy_type_matrix.rs"]
mod binary_copy_type_matrix;
#[path = "constraint_ingress.rs"]
mod constraint_ingress;
#[path = "force_null_csv_markers.rs"]
mod force_null_csv_markers;
#[path = "header_match_rejections.rs"]
mod header_match_rejections;
#[path = "forms.rs"]
mod issue_846_residual_forms;
#[path = "legacy_syntax.rs"]
mod legacy_syntax;
#[path = "materialized_view_to.rs"]
mod materialized_view_to;
#[path = "pgwire_cancellation.rs"]
mod pgwire_cancellation;
#[path = "from_stdin.rs"]
mod postgresql18_from_stdin_residuals;
#[path = "query_to.rs"]
mod query_to;
#[path = "text_format.rs"]
mod text_format;
#[path = "trigger_interactions.rs"]
mod trigger_interactions;
#[path = "missing_forms.rs"]
mod wave4_missing_forms;

#[test]
fn should_filter_copy_from_stdin_rows_with_copy_where() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_where").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_where (id INT, label TEXT)")
            .await
            .expect("create table for COPY WHERE");

        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        support_pgwire::complete_startup(&mut reader, &mut writer).await;

        // Act
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY compat_copy_where (id, label) FROM STDIN \
                 WITH (FORMAT csv) WHERE (id > 1)",
            )],
        )
        .await;
        let (copy_response_tag, copy_response_payload) =
            support_pgwire::read_wire_frame(&mut reader).await;
        let (mut copy_frames, copied_command) = if copy_response_tag == b'G' {
            support_pgwire::write_frames(
                &mut writer,
                vec![
                    support_pgwire::copy_data_frame(b"1,skip\n2,keep\n"),
                    support_pgwire::copy_done_frame(),
                ],
            )
            .await;
            let frames = support_pgwire::read_frames_until_ready_within(
                &mut reader,
                Duration::from_secs(5),
            )
            .await;
            let command = frames
                .iter()
                .find(|(tag, _)| *tag == b'C')
                .map(|(_, payload)| {
                    String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload))
                        .into_owned()
                });
            (frames, command)
        } else {
            (Vec::new(), None)
        };
        copy_frames.insert(
            0,
            (copy_response_tag, copy_response_payload.clone()),
        );
        let rows = client
            .query(
                "SELECT id, label FROM compat_copy_where ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after COPY WHERE");

        // Assert
        let error_code = support_pgwire::error_code(&copy_frames);
        drop((reader, writer, client));
        server.shutdown(connection).await;

        assert_eq!(
            copy_response_tag,
            b'G',
            "COPY WHERE should enter COPY IN; payload: {copy_response_payload:?}; SQLSTATE: {error_code:?}"
        );
        assert_eq!(copied_command.as_deref(), Some("COPY 1"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 2);
        assert_eq!(rows[0].get::<_, String>(1), "keep");
    });
}

#[test]
fn should_match_copy_csv_header_to_target_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_header_match").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_header_match (id INT, label TEXT)")
            .await
            .expect("create table for COPY HEADER MATCH");

        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        support_pgwire::complete_startup(&mut reader, &mut writer).await;

        // Act
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY compat_copy_header_match (id, label) FROM STDIN \
                 WITH (FORMAT csv, HEADER MATCH)",
            )],
        )
        .await;
        let (copy_response_tag, copy_response_payload) =
            support_pgwire::read_wire_frame(&mut reader).await;
        let (mut copy_frames, copied_command) = if copy_response_tag == b'G' {
            support_pgwire::write_frames(
                &mut writer,
                vec![
                    support_pgwire::copy_data_frame(b"id,label\n1,keep\n"),
                    support_pgwire::copy_done_frame(),
                ],
            )
            .await;
            let frames =
                support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                    .await;
            let command = frames
                .iter()
                .find(|(tag, _)| *tag == b'C')
                .map(|(_, payload)| {
                    String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload))
                        .into_owned()
                });
            (frames, command)
        } else {
            (Vec::new(), None)
        };
        copy_frames.insert(0, (copy_response_tag, copy_response_payload.clone()));
        let rows = client
            .query(
                "SELECT id, label FROM compat_copy_header_match ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after COPY HEADER MATCH");

        // Assert
        let error_code = support_pgwire::error_code(&copy_frames);
        drop((reader, writer, client));
        server.shutdown(connection).await;

        assert_eq!(
            copy_response_tag, b'G',
            "COPY HEADER MATCH should enter COPY IN; payload: {copy_response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(copied_command.as_deref(), Some("COPY 1"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "keep");
    });
}

async fn copy_from_stdin(
    server: &CompatibilityServer,
    sql: &str,
    data: &[u8],
) -> (u8, Vec<u8>, Option<String>, Option<String>) {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw pgwire COPY client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    let (response_tag, response_payload) = support_pgwire::read_wire_frame(&mut reader).await;
    if response_tag == b'G' {
        support_pgwire::write_frames(
            &mut writer,
            vec![
                support_pgwire::copy_data_frame(data),
                support_pgwire::copy_done_frame(),
            ],
        )
        .await;
    }
    let mut frames =
        support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    frames.insert(0, (response_tag, response_payload.clone()));
    let error_code = support_pgwire::error_code(&frames);
    let command = frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
        });
    drop((reader, writer));
    (response_tag, response_payload, error_code, command)
}

#[test]
fn should_import_csv_with_custom_delimiter() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_csv_delimiter").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_delimiter (id INT, note TEXT)")
            .await
            .expect("create table for a custom CSV delimiter");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_delimiter FROM STDIN WITH (FORMAT CSV, DELIMITER '|')",
            b"1|plain\n2|\"a|b\"\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query(
                    "SELECT id, note FROM compat_copy_delimiter ORDER BY id",
                    &[],
                )
                .await
                .expect("read imported CSV rows")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "custom delimiter should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(1, "plain".to_string()), (2, "a|b".to_string())]
        );
    });
}

#[test]
fn should_import_csv_with_custom_null_marker() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_csv_null_marker").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_null_marker (id INT, note TEXT)")
            .await
            .expect("create table for a custom CSV NULL marker");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_null_marker FROM STDIN WITH (FORMAT CSV, NULL 'N')",
            b"1,N\n2,\"N\"\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query(
                    "SELECT id, note FROM compat_copy_null_marker ORDER BY id",
                    &[],
                )
                .await
                .expect("read imported CSV NULL fields")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "custom NULL marker should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(rows[0].get::<_, Option<String>>(1), None);
        assert_eq!(rows[1].get::<_, Option<String>>(1).as_deref(), Some("N"));
    });
}

#[test]
fn should_keep_empty_csv_fields_with_force_not_null() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_csv_force_not_null").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_force_not_null (id INT, note TEXT)")
            .await
            .expect("create table for FORCE_NOT_NULL");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_force_not_null FROM STDIN \
             WITH (FORMAT CSV, FORCE_NOT_NULL (note))",
            b"1,\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query(
                    "SELECT note FROM compat_copy_force_not_null WHERE id = 1",
                    &[],
                )
                .await
                .expect("read imported CSV field")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "FORCE_NOT_NULL should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert_eq!(rows[0].get::<_, Option<String>>(0).as_deref(), Some(""));
    });
}

#[test]
fn should_force_quoted_empty_csv_fields_to_null() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_csv_force_null").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_copy_force_null \
                   (id INT, forced TEXT, normal TEXT)",
            )
            .await
            .expect("create table for FORCE_NULL");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_force_null FROM STDIN WITH (FORMAT CSV, FORCE_NULL (forced))",
            b"1,\"\",\"\"\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query(
                    "SELECT forced, normal FROM compat_copy_force_null WHERE id = 1",
                    &[],
                )
                .await
                .expect("read imported CSV fields")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "FORCE_NULL should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert_eq!(rows[0].get::<_, Option<String>>(0), None);
        assert_eq!(rows[0].get::<_, Option<String>>(1).as_deref(), Some(""));
    });
}

#[test]
fn should_parse_custom_csv_quote_character() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_csv_quote").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_quote (id INT, note TEXT)")
            .await
            .expect("create table for a custom CSV quote character");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_quote FROM STDIN WITH (FORMAT CSV, QUOTE '~')",
            b"1,~a,b~\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query("SELECT note FROM compat_copy_quote WHERE id = 1", &[])
                .await
                .expect("read imported CSV field")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "custom quote should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert_eq!(rows[0].get::<_, String>(0), "a,b");
    });
}

#[test]
fn should_parse_custom_csv_escape_character() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_csv_escape").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_escape (id INT, note TEXT)")
            .await
            .expect("create table for a custom CSV escape character");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_escape FROM STDIN WITH (FORMAT CSV, ESCAPE '!')",
            b"1,\"a!\"b\"\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query("SELECT note FROM compat_copy_escape WHERE id = 1", &[])
                .await
                .expect("read imported CSV field")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "custom escape should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert_eq!(rows[0].get::<_, String>(0), "a\"b");
    });
}

#[test]
fn should_skip_invalid_rows_with_copy_on_error_ignore() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_on_error_ignore").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_on_error (id INT, note TEXT)")
            .await
            .expect("create table for COPY ON_ERROR");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_on_error FROM STDIN WITH (FORMAT CSV, ON_ERROR ignore)",
            b"1,first\nnot-an-integer,discard\n2,last\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query("SELECT id, note FROM compat_copy_on_error ORDER BY id", &[])
                .await
                .expect("read accepted rows after COPY")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "ON_ERROR ignore should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[1].get::<_, i32>(0), 2);
    });
}

#[test]
fn should_apply_copy_default_string_to_table_defaults() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_default_string").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_copy_default_string \
                   (id INT, note TEXT DEFAULT 'fallback')",
            )
            .await
            .expect("create table with a column default");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_default_string FROM STDIN \
             WITH (FORMAT CSV, DEFAULT 'DEFAULT')",
            b"1,DEFAULT\n2,provided\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query(
                    "SELECT id, note FROM compat_copy_default_string ORDER BY id",
                    &[],
                )
                .await
                .expect("read imported rows after default substitution")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "COPY DEFAULT should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(rows[0].get::<_, String>(1), "fallback");
        assert_eq!(rows[1].get::<_, String>(1), "provided");
    });
}

#[test]
fn should_abort_copy_when_reject_limit_is_exceeded() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_reject_limit").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_reject_limit (id INT, note TEXT)")
            .await
            .expect("create table for COPY REJECT_LIMIT");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_reject_limit FROM STDIN \
             WITH (FORMAT CSV, REJECT_LIMIT 1, ON_ERROR ignore)",
            b"1,first\nnot-an-integer,discarded\n2,last\nalso-not-an-integer,discarded-too\n3,third\n",
        )
        .await;
        let rows = client
            .query("SELECT count(*) FROM compat_copy_reject_limit", &[])
            .await
            .expect("read the relation after COPY");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag,
            b'G',
            "REJECT_LIMIT should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code.as_deref(), Some("22P02"));
        assert!(command.is_none());
        assert_eq!(rows[0].get::<_, i64>(0), 0);
    });
}

#[test]
fn should_decode_copy_data_using_declared_encoding() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_declared_encoding").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_encoding (id INT, note TEXT)")
            .await
            .expect("create table for COPY ENCODING");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_copy_encoding FROM STDIN \
             WITH (FORMAT CSV, ENCODING 'LATIN1')",
            b"1,caf\xe9\n",
        )
        .await;
        let rows = if response_tag == b'G' {
            client
                .query("SELECT note FROM compat_copy_encoding WHERE id = 1", &[])
                .await
                .expect("read decoded text field")
        } else {
            Vec::new()
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "COPY ENCODING should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert_eq!(rows[0].get::<_, String>(0), "café");
    });
}
