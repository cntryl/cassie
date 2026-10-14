use super::*;

async fn copy_out_frames(server: &CompatibilityServer, sql: &str) -> Vec<(u8, Vec<u8>)> {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw pgwire COPY client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await
}

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

fn copy_data(frames: &[(u8, Vec<u8>)]) -> Vec<u8> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'd')
        .flat_map(|(_, payload)| payload.iter().copied())
        .collect()
}

#[test]
fn should_enforce_server_copy_endpoint_privileges() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_copy_endpoint_roles").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE ROLE compat_issue846_copy_unprivileged NOLOGIN; \
                 CREATE TABLE compat_issue846_copy_policy (id INT); \
                 INSERT INTO compat_issue846_copy_policy VALUES (1); \
                 GRANT SELECT, INSERT ON compat_issue846_copy_policy \
                   TO compat_issue846_copy_unprivileged; \
                 SET ROLE compat_issue846_copy_unprivileged",
            )
            .await
            .expect("create an unprivileged role with table access");

        // Act
        let cases = [
            (
                "COPY compat_issue846_copy_policy FROM '/dev/null' WITH (FORMAT CSV)",
                "permission denied to COPY from a file",
                "pg_read_server_files",
            ),
            (
                "COPY compat_issue846_copy_policy TO '/dev/null' WITH (FORMAT CSV)",
                "permission denied to COPY to a file",
                "pg_write_server_files",
            ),
            (
                "COPY compat_issue846_copy_policy FROM PROGRAM 'true' WITH (FORMAT CSV)",
                "permission denied to COPY to or from an external program",
                "pg_execute_server_program",
            ),
            (
                "COPY compat_issue846_copy_policy TO PROGRAM 'true' WITH (FORMAT CSV)",
                "permission denied to COPY to or from an external program",
                "pg_execute_server_program",
            ),
        ];
        let mut errors = Vec::with_capacity(cases.len());
        for (sql, _, _) in cases {
            errors.push(
                client
                    .batch_execute(sql)
                    .await
                    .expect_err("unprivileged server-side COPY must be rejected"),
            );
        }
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative session role");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        for ((_, expected_message, expected_role), error) in cases.into_iter().zip(errors) {
            let database_error = db_error(&error);
            assert_eq!(database_error.code().code(), "42501");
            assert_eq!(database_error.message(), expected_message);
            assert!(
                database_error
                    .detail()
                    .expect("COPY privilege detail")
                    .contains(expected_role),
                "detail must identify {expected_role}: {database_error:?}"
            );
        }
    });
}

#[test]
fn should_reject_copy_freeze_for_a_table_from_an_earlier_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_copy_freeze_precondition").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_issue846_copy_freeze_old (id INT)")
            .await
            .expect("create the target table in its own transaction");

        // Act
        let frames = copy_in_frames(
            &server,
            "COPY compat_issue846_copy_freeze_old FROM STDIN WITH (FREEZE)",
            b"1\n",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let command = copy_command(&frames);
        let error_message = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .and_then(|(_, payload)| {
                support_pgwire::parse_error_fields(payload)
                    .into_iter()
                    .find(|(kind, _)| *kind == 'M')
                    .map(|(_, message)| message)
            });
        let row_count = client
            .query_one(
                "SELECT count(*) FROM compat_issue846_copy_freeze_old",
                &[],
            )
            .await
            .expect("read the table after rejected COPY FREEZE")
            .get::<_, i64>(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(error_code.as_deref(), Some("55000"));
        assert_eq!(command, None);
        assert_eq!(
            error_message.as_deref(),
            Some(
                "cannot perform COPY FREEZE because the table was not created or truncated in the current subtransaction"
            )
        );
        assert_eq!(row_count, 0);
    });
}

#[test]
fn should_accept_copy_freeze_for_a_table_created_in_the_same_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_copy_freeze_valid").await;
        let (client, connection) = server.connect().await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        support_pgwire::complete_startup(&mut reader, &mut writer).await;
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "BEGIN; CREATE TEMP TABLE compat_issue846_copy_freeze_new \
                 (id INT, label TEXT)",
            )],
        )
        .await;
        let setup_frames =
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await;
        let setup_error = support_pgwire::error_code(&setup_frames);

        // Act
        let mut copy_response_tag = 0;
        let mut copy_response_payload = Vec::new();
        let mut copy_frames = Vec::new();
        let mut selected_rows = Vec::new();
        if setup_error.is_none() {
            support_pgwire::write_frames(
                &mut writer,
                vec![support_pgwire::simple_query_frame(
                    "COPY compat_issue846_copy_freeze_new FROM STDIN \
                     WITH (FORMAT CSV, FREEZE)",
                )],
            )
            .await;
            let response = support_pgwire::read_wire_frame(&mut reader).await;
            copy_response_tag = response.0;
            copy_response_payload = response.1;
            if copy_response_tag == b'G' {
                support_pgwire::write_frames(
                    &mut writer,
                    vec![
                        support_pgwire::copy_data_frame(b"1,first\n2,second\n"),
                        support_pgwire::copy_done_frame(),
                    ],
                )
                .await;
            }
            copy_frames =
                support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                    .await;
            copy_frames.insert(0, (copy_response_tag, copy_response_payload.clone()));
            if support_pgwire::error_code(&copy_frames).is_none() && copy_response_tag == b'G' {
                support_pgwire::write_frames(
                    &mut writer,
                    vec![support_pgwire::simple_query_frame(
                        "SELECT id, label FROM compat_issue846_copy_freeze_new ORDER BY id",
                    )],
                )
                .await;
                let rows = support_pgwire::read_frames_until_ready_within(
                    &mut reader,
                    Duration::from_secs(5),
                )
                .await;
                selected_rows = support_pgwire::data_rows(&rows);
            }
            support_pgwire::write_frames(
                &mut writer,
                vec![support_pgwire::simple_query_frame("ROLLBACK")],
            )
            .await;
            let _ =
                support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                    .await;
        }
        let copy_error = support_pgwire::error_code(&copy_frames);
        let copied_command = copy_command(&copy_frames);

        // Assert
        drop((reader, writer, client));
        server.shutdown(connection).await;
        assert_eq!(
            setup_error, None,
            "transaction and table setup: {setup_frames:?}"
        );
        assert_eq!(
            copy_response_tag, b'G',
            "valid COPY FREEZE should enter COPY IN; payload: {copy_response_payload:?}; \
             SQLSTATE: {copy_error:?}"
        );
        assert_eq!(copy_error, None);
        assert_eq!(copied_command.as_deref(), Some("COPY 2"));
        assert_eq!(
            selected_rows,
            vec![
                vec![Some("1".to_owned()), Some("first".to_owned())],
                vec![Some("2".to_owned()), Some("second".to_owned())],
            ]
        );
    });
}

#[test]
fn should_import_legacy_csv_alias_option_matrix() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_legacy_csv_aliases").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_issue846_legacy_csv \
                 (id INT, nullable_note TEXT, forced_note TEXT)",
            )
            .await
            .expect("create the legacy CSV target table");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_issue846_legacy_csv FROM STDIN WITH CSV \
             DELIMITER AS '|' NULL AS 'N' QUOTE AS '~' ESCAPE AS '!' \
             FORCE NOT NULL forced_note",
            b"1|N|N\n2|N|~a!~b~\n3|plain|value\n",
        )
        .await;
        let rows = if response_tag == b'G' && error_code.is_none() {
            Some(
                client
                    .query(
                        "SELECT id, nullable_note, forced_note \
                         FROM compat_issue846_legacy_csv ORDER BY id",
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
            "legacy CSV aliases should enter COPY IN; payload: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 3"));
        let rows = rows
            .expect("query imported rows only after COPY completed")
            .expect("read imported legacy CSV rows");
        assert_eq!(
            rows.iter()
                .map(|row| (
                    row.get::<_, i32>(0),
                    row.get::<_, Option<String>>(1),
                    row.get::<_, String>(2),
                ))
                .collect::<Vec<_>>(),
            vec![
                (1, None, "N".to_owned()),
                (2, None, "a~b".to_owned()),
                (3, Some("plain".to_owned()), "value".to_owned()),
            ]
        );
    });
}

#[test]
fn should_import_pre_73_using_delimiters_with_null_alias() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_pre_73_copy_aliases").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_issue846_using_delimiters (id INT, note TEXT)")
            .await
            .expect("create the pre-7.3 COPY target table");

        // Act
        let (response_tag, response_payload, error_code, command) = copy_from_stdin(
            &server,
            "COPY compat_issue846_using_delimiters FROM STDIN \
             USING DELIMITERS '|' WITH NULL AS 'N'",
            b"1|plain\n2|N\n",
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
            "pre-7.3 USING DELIMITERS should enter COPY IN; payload: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 2"));
        let rows = rows
            .expect("query imported rows only after COPY completed")
            .expect("read imported pre-7.3 COPY rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "plain");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert!(rows[1].get::<_, Option<String>>(1).is_none());
    });
}

#[test]
fn should_export_csv_with_legacy_force_quote_alias() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_legacy_force_quote").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_issue846_force_quote (id INT, note TEXT); \
                 INSERT INTO compat_issue846_force_quote VALUES (1, 'plain'), (2, 'a,b')",
            )
            .await
            .expect("create and populate the CSV output table");

        // Act
        let frames = copy_out_frames(
            &server,
            "COPY compat_issue846_force_quote TO STDOUT WITH CSV FORCE QUOTE note",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let output = String::from_utf8(copy_data(&frames)).expect("COPY CSV output is UTF-8");
        let command = copy_command(&frames);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(error_code, None, "COPY CSV output frames: {frames:?}");
        assert!(
            frames.iter().any(|(tag, _)| *tag == b'H'),
            "COPY TO should begin with CopyOutResponse: {frames:?}"
        );
        assert!(frames.iter().any(|(tag, _)| *tag == b'c'));
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(output, "1,\"plain\"\n2,\"a,b\"\n");
    });
}

#[test]
fn should_export_binary_with_both_legacy_copy_spellings() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_legacy_binary").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_issue846_legacy_binary (id INT); \
                 INSERT INTO compat_issue846_legacy_binary VALUES (42)",
            )
            .await
            .expect("create and populate the binary COPY output table");

        // Act
        let leading_binary = copy_out_frames(
            &server,
            "COPY BINARY compat_issue846_legacy_binary TO STDOUT",
        )
        .await;
        let with_binary = copy_out_frames(
            &server,
            "COPY compat_issue846_legacy_binary TO STDOUT WITH BINARY",
        )
        .await;
        let leading_error = support_pgwire::error_code(&leading_binary);
        let with_error = support_pgwire::error_code(&with_binary);
        let leading_payload = copy_data(&leading_binary);
        let with_payload = copy_data(&with_binary);
        let leading_command = copy_command(&leading_binary);
        let with_command = copy_command(&with_binary);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            leading_error, None,
            "leading BINARY frames: {leading_binary:?}"
        );
        assert_eq!(with_error, None, "WITH BINARY frames: {with_binary:?}");
        assert_eq!(
            leading_binary
                .iter()
                .find(|(tag, _)| *tag == b'H')
                .map(|(_, payload)| payload.as_slice()),
            Some([1_u8, 0, 1, 0, 1].as_slice())
        );
        assert_eq!(
            with_binary
                .iter()
                .find(|(tag, _)| *tag == b'H')
                .map(|(_, payload)| payload.as_slice()),
            Some([1_u8, 0, 1, 0, 1].as_slice())
        );
        assert!(leading_binary.iter().any(|(tag, _)| *tag == b'c'));
        assert!(with_binary.iter().any(|(tag, _)| *tag == b'c'));
        assert_eq!(leading_command.as_deref(), Some("COPY 1"));
        assert_eq!(with_command.as_deref(), Some("COPY 1"));

        let mut expected = b"PGCOPY\n\xff\r\n\0".to_vec();
        expected.extend_from_slice(&0_i32.to_be_bytes());
        expected.extend_from_slice(&0_i32.to_be_bytes());
        expected.extend_from_slice(&1_i16.to_be_bytes());
        expected.extend_from_slice(&4_i32.to_be_bytes());
        expected.extend_from_slice(&42_i32.to_be_bytes());
        expected.extend_from_slice(&(-1_i16).to_be_bytes());
        assert_eq!(leading_payload, expected);
        assert_eq!(with_payload, expected);
    });
}
