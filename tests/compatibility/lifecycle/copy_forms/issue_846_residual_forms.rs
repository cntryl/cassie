use super::*;

async fn copy_in_frames(
    server: &CompatibilityServer,
    sql: &str,
    data: &[u8],
) -> Vec<(u8, Vec<u8>)> {
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
    frames.insert(0, (response_tag, response_payload));
    frames
}

fn copy_command(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
        })
}

#[test]
fn should_preserve_csv_null_marker_for_force_not_null_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_copy_force_not_null").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_issue846_copy_force_not_null \
                 (id INT, ordinary TEXT, forced TEXT)",
            )
            .await
            .expect("create a CSV COPY target with one forced text column");

        // Verify the standard COPY FROM STDIN path before the focused option case.
        let control = copy_in_frames(
            &server,
            "COPY compat_issue846_copy_force_not_null FROM STDIN WITH (FORMAT CSV)",
            b"0,baseline,baseline\n",
        )
        .await;
        assert_eq!(
            control.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "standard CSV COPY control enters COPY IN: {control:?}"
        );
        assert_eq!(support_pgwire::error_code(&control), None);
        assert_eq!(copy_command(&control).as_deref(), Some("COPY 1"));

        // Act
        let frames = copy_in_frames(
            &server,
            "COPY compat_issue846_copy_force_not_null (id, ordinary, forced) FROM STDIN \
             WITH (FORMAT CSV, FORCE_NOT_NULL (forced))",
            b"1,,\n2,plain,plain\n",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let command = copy_command(&frames);
        let rows = client
            .query(
                "SELECT id, ordinary, forced \
                 FROM compat_issue846_copy_force_not_null ORDER BY id",
                &[],
            )
            .await
            .expect("read values imported with FORCE_NOT_NULL");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            frames.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "PostgreSQL 18.6 accepts FORCE_NOT_NULL and enters COPY IN: {frames:?}"
        );
        assert_eq!(error_code, None, "FORCE_NOT_NULL SQLSTATE: {frames:?}");
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(
            rows.iter()
                .map(|row| (
                    row.get::<_, i32>(0),
                    row.get::<_, Option<String>>(1),
                    row.get::<_, Option<String>>(2),
                ))
                .collect::<Vec<_>>(),
            vec![
                (0, Some("baseline".to_owned()), Some("baseline".to_owned())),
                (1, None, Some(String::new())),
                (2, Some("plain".to_owned()), Some("plain".to_owned())),
            ]
        );
    });
}

#[test]
fn should_apply_copy_default_marker_without_changing_csv_nulls() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_copy_default_marker").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_issue846_copy_default_marker \
                 (id INT, payload TEXT DEFAULT 'fresh')",
            )
            .await
            .expect("create a COPY target with a text default");

        // Verify the standard COPY FROM STDIN path before the focused option case.
        let control = copy_in_frames(
            &server,
            "COPY compat_issue846_copy_default_marker FROM STDIN WITH (FORMAT CSV)",
            b"0,baseline\n",
        )
        .await;
        assert_eq!(
            control.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "standard CSV COPY control enters COPY IN: {control:?}"
        );
        assert_eq!(support_pgwire::error_code(&control), None);
        assert_eq!(copy_command(&control).as_deref(), Some("COPY 1"));

        // Act
        let frames = copy_in_frames(
            &server,
            "COPY compat_issue846_copy_default_marker (id, payload) FROM STDIN \
             WITH (FORMAT CSV, DEFAULT 'DEFAULT')",
            b"1,DEFAULT\n2,\n3,actual\n",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let command = copy_command(&frames);
        let rows = client
            .query(
                "SELECT id, payload FROM compat_issue846_copy_default_marker ORDER BY id",
                &[],
            )
            .await
            .expect("read imported DEFAULT marker and null values");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            frames.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "PostgreSQL 18.6 accepts COPY DEFAULT marker and enters COPY IN: {frames:?}"
        );
        assert_eq!(error_code, None, "COPY DEFAULT SQLSTATE: {frames:?}");
        assert_eq!(command.as_deref(), Some("COPY 3"));
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, Option<String>>(1),))
                .collect::<Vec<_>>(),
            vec![
                (0, Some("baseline".to_owned())),
                (1, Some("fresh".to_owned())),
                (2, None),
                (3, Some("actual".to_owned())),
            ]
        );
    });
}

#[test]
fn should_import_postgresql_dump_default_text_copy_data_section() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_pg_dump_text_copy").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_issue846_pg_dump_text_copy (payload TEXT)")
            .await
            .expect("create a target for PostgreSQL's default-text COPY section");

        // Act
        // PostgreSQL 18.6 default-text COPY decodes the tab escape and treats \\N as NULL.
        let frames = copy_in_frames(
            &server,
            "COPY compat_issue846_pg_dump_text_copy FROM STDIN",
            b"alpha\\tbeta\n\\N\n",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let command = copy_command(&frames);
        let rows = client
            .query(
                "SELECT payload FROM compat_issue846_pg_dump_text_copy \
                 ORDER BY payload NULLS LAST",
                &[],
            )
            .await
            .expect("read the imported pg_dump-style text rows");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            frames.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "PostgreSQL 18.6 default-text COPY enters COPY IN: {frames:?}"
        );
        assert_eq!(error_code, None, "pg_dump text COPY SQLSTATE: {frames:?}");
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(
            rows.iter()
                .map(|row| row.get::<_, Option<String>>(0))
                .collect::<Vec<_>>(),
            vec![Some("alpha\tbeta".to_owned()), None]
        );
    });
}

#[test]
fn should_import_copy_with_legacy_using_delimiters() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_pre_73_copy_aliases").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_issue846_using_delimiters (id INT, note TEXT)")
            .await
            .expect("create the pre-7.3 COPY target table");

        // Verify standard COPY FROM STDIN before the legacy delimiter form.
        let control = copy_in_frames(
            &server,
            "COPY compat_issue846_using_delimiters FROM STDIN WITH (FORMAT CSV)",
            b"0,baseline\n",
        )
        .await;
        assert_eq!(
            control.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "standard COPY control enters COPY IN: {control:?}"
        );
        assert_eq!(support_pgwire::error_code(&control), None);
        assert_eq!(copy_command(&control).as_deref(), Some("COPY 1"));

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_issue846_using_delimiters FROM STDIN USING DELIMITERS '|'",
            b"1|plain\n",
        )
        .await;
        let rows = if response_tag == b'G' && error_code.is_none() {
            Some(
                client
                    .query(
                        "SELECT id, note FROM compat_issue846_using_delimiters ORDER BY id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "PostgreSQL 18.6 USING DELIMITERS should enter COPY IN; payload: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 1"));
        let rows = rows
            .expect("query imported rows only after COPY completed")
            .expect("read imported legacy COPY rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 0);
        assert_eq!(rows[0].get::<_, String>(1), "baseline");
        assert_eq!(rows[1].get::<_, i32>(0), 1);
        assert_eq!(rows[1].get::<_, String>(1), "plain");
    });
}
