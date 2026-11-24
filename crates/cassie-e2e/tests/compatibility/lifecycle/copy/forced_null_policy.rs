use super::*;

#[test]
fn should_preserve_csv_null_marker_origin_given_force_options_when_columns_are_reordered() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_forced_null_origin").await;
        let (client, connection) = server.connect().await;
        let cases = [
            ("N", "", [None, Some("N"), Some(""), Some("")]),
            (
                "N",
                ", FORCE_NULL (\"Value\")",
                [None, None, Some(""), Some("")],
            ),
            (
                "N",
                ", FORCE_NOT_NULL (\"Value\")",
                [Some("N"), Some("N"), Some(""), Some("")],
            ),
            (
                "N",
                ", FORCE_NULL (\"Value\"), FORCE_NOT_NULL (\"Value\")",
                [Some("N"), None, Some(""), Some("")],
            ),
            ("", "", [Some("N"), Some("N"), None, Some("")]),
            (
                "",
                ", FORCE_NULL (\"Value\")",
                [Some("N"), Some("N"), None, None],
            ),
            (
                "",
                ", FORCE_NOT_NULL (\"Value\")",
                [Some("N"), Some("N"), Some(""), Some("")],
            ),
            (
                "",
                ", FORCE_NULL (\"Value\"), FORCE_NOT_NULL (\"Value\")",
                [Some("N"), Some("N"), Some(""), None],
            ),
            ("N", ", FORCE_NULL *", [None, None, Some(""), Some("")]),
            (
                "N",
                ", FORCE_NOT_NULL *",
                [Some("N"), Some("N"), Some(""), Some("")],
            ),
        ];
        let mut observations = Vec::new();

        // Act
        for (index, (marker, options, expected)) in cases.into_iter().enumerate() {
            let table = format!("copy_forced_origin_{index}");
            client
                .batch_execute(&format!(
                    "CREATE TABLE {table} (id INT, \"Value\" TEXT DEFAULT 'fallback')"
                ))
                .await
                .expect("create reordered COPY target");
            let sql = format!(
                "COPY {table}(\"Value\",id) FROM STDIN WITH (FORMAT CSV, NULL '{marker}'{options})"
            );
            let (tag, _, error, command) =
                super::copy_from_stdin(&server, &sql, b"N,1\n\"N\",2\n,3\n\"\",4\n").await;
            let rows = client
                .query(&format!("SELECT \"Value\" FROM {table} ORDER BY id"), &[])
                .await
                .expect("read forced NULL rows");
            let actual = rows
                .iter()
                .map(|row| row.get::<_, Option<String>>(0))
                .collect::<Vec<_>>();
            observations.push((sql, expected, tag, error, command, actual));
        }
        drop(client);
        server.shutdown(connection).await;

        // Assert
        for (sql, expected, tag, error, command, actual) in observations {
            assert_eq!(tag, b'G', "{sql}: {error:?}");
            assert_eq!(error, None, "{sql}");
            assert_eq!(command.as_deref(), Some("COPY 4"), "{sql}");
            assert_eq!(
                actual,
                expected.map(|value| value.map(str::to_owned)),
                "{sql}"
            );
        }
    });
}

#[test]
fn should_apply_star_force_options_given_an_implicit_copy_column_list() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_forced_star_implicit").await;
        let (client, connection) = server.connect().await;
        let cases = [
            (", FORCE_NULL *", [None, None]),
            (", FORCE_NOT_NULL *", [Some("N"), Some("N")]),
            (", FORCE_NULL *, FORCE_NOT_NULL *", [Some("N"), None]),
        ];
        let mut observations = Vec::new();

        // Act
        for (index, (options, expected)) in cases.into_iter().enumerate() {
            let table = format!("copy_forced_star_implicit_{index}");
            client
                .batch_execute(&format!(
                    "CREATE TABLE {table} (id INT, v TEXT DEFAULT 'fallback')"
                ))
                .await
                .expect("create implicit-column target");
            let (tag, _, state, command) = super::copy_from_stdin(
                &server,
                &format!("COPY {table} FROM STDIN WITH (FORMAT CSV, NULL 'N'{options})"),
                b"1,N\n2,\"N\"\n",
            )
            .await;
            let rows = client
                .query(&format!("SELECT v FROM {table} ORDER BY id"), &[])
                .await
                .expect("read implicitly targeted values");
            let actual = rows
                .iter()
                .map(|row| row.get::<_, Option<String>>(0))
                .collect::<Vec<_>>();
            observations.push((expected, tag, state, command, actual));
        }
        drop(client);
        server.shutdown(connection).await;

        // Assert
        for (expected, tag, state, command, actual) in observations {
            assert_eq!(tag, b'G', "{state:?}");
            assert_eq!(state, None);
            assert_eq!(command.as_deref(), Some("COPY 2"));
            assert_eq!(actual, expected.map(|value| value.map(str::to_owned)));
        }
    });
}

#[test]
fn should_reject_invalid_csv_force_targets_before_entering_copy_mode() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_forced_null_validation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE copy_forced_validation (id INT, v TEXT)")
            .await
            .expect("create validation target");
        let cases = [
            (
                "COPY copy_forced_validation(id) FROM STDIN WITH (FORMAT CSV, FORCE_NULL(v))",
                "42P10",
            ),
            (
                "COPY copy_forced_validation FROM STDIN WITH (FORMAT CSV, FORCE_NULL(missing))",
                "42703",
            ),
            (
                "COPY copy_forced_validation FROM STDIN WITH (FORMAT CSV, FORCE_NULL(v,v))",
                "42701",
            ),
            (
                "COPY copy_forced_validation FROM STDIN WITH (FORMAT TEXT, FORCE_NULL(v))",
                "0A000",
            ),
            (
                "COPY copy_forced_validation TO STDOUT WITH (FORMAT CSV, FORCE_NULL(v))",
                "22023",
            ),
        ];
        let mut observations = Vec::new();

        // Act
        for (sql, expected) in cases {
            let (tag, payload, state, command) = super::copy_from_stdin(&server, sql, b"").await;
            observations.push((sql, expected, tag, payload, state, command));
        }
        drop(client);
        server.shutdown(connection).await;

        // Assert
        for (sql, expected, tag, payload, state, command) in observations {
            assert_eq!(tag, b'E', "{sql} must reject before streaming");
            assert_eq!(state.as_deref(), Some(expected), "{sql}: {payload:?}");
            assert_eq!(command, None, "{sql}");
        }
    });
}

#[test]
fn should_preserve_json_sql_null_given_force_options_when_a_column_has_a_default() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_forced_json_null_default").await;
        let (client, connection) = server.connect().await;
        let cases = [
            ("", [true, false]),
            (", FORCE_NULL(v)", [true, true]),
            (", FORCE_NOT_NULL(v)", [false, false]),
            (", FORCE_NULL(v), FORCE_NOT_NULL(v)", [false, true]),
        ];
        let mut observations = Vec::new();

        // Act
        for (index, (options, expected)) in cases.into_iter().enumerate() {
            let table = format!("copy_forced_json_{index}");
            client
                .batch_execute(&format!(
                    "CREATE TABLE {table} (id INT, v JSONB DEFAULT '{{\"default\":1}}'::jsonb)"
                ))
                .await
                .expect("create defaulted JSON target");
            let (tag, _, state, command) = super::copy_from_stdin(
                &server,
                &format!("COPY {table}(v,id) FROM STDIN WITH (FORMAT CSV, NULL 'null'{options})"),
                b"null,1\n\"null\",2\n",
            )
            .await;
            let (_, _, omitted_error, omitted_command) = super::copy_from_stdin(
                &server,
                &format!("COPY {table}(id) FROM STDIN WITH (FORMAT CSV)"),
                b"3\n",
            )
            .await;
            let rows = client
                .query(
                    &format!("SELECT v IS NULL,v::text FROM {table} ORDER BY id"),
                    &[],
                )
                .await
                .expect("read copied JSON values");
            let actual = rows
                .iter()
                .map(|row| (row.get::<_, bool>(0), row.get::<_, Option<String>>(1)))
                .collect::<Vec<_>>();
            observations.push((
                expected,
                tag,
                state,
                command,
                omitted_error,
                omitted_command,
                actual,
            ));
        }
        drop(client);
        server.shutdown(connection).await;

        // Assert
        for (expected, tag, state, command, omitted_error, omitted_command, actual) in observations
        {
            assert_eq!(tag, b'G', "{state:?}");
            assert_eq!(state, None);
            assert_eq!(command.as_deref(), Some("COPY 2"));
            assert_eq!(omitted_error, None);
            assert_eq!(omitted_command.as_deref(), Some("COPY 1"));
            assert_eq!(
                actual,
                vec![
                    (expected[0], (!expected[0]).then(|| "null".to_owned())),
                    (expected[1], (!expected[1]).then(|| "null".to_owned())),
                    (false, Some("{\"default\": 1}".to_owned())),
                ]
            );
        }
    });
}

#[test]
fn should_allocate_sequence_defaults_once_given_forced_json_null_and_omitted_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_forced_json_sequence").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE copy_forced_json_sequence (id INT, v JSONB DEFAULT '{\"default\":1}'::jsonb, seq SERIAL UNIQUE)",
            )
            .await
            .expect("create JSON and sequence defaults");

        // Act
        let copied = super::copy_from_stdin(
            &server,
            "COPY copy_forced_json_sequence(v,id) FROM STDIN WITH (FORMAT CSV, NULL 'null', FORCE_NULL(v), FORCE_NOT_NULL(v))",
            b"null,1\n\"null\",2\n",
        )
        .await;
        let omitted = super::copy_from_stdin(
            &server,
            "COPY copy_forced_json_sequence(id) FROM STDIN WITH (FORMAT CSV)",
            b"3\n",
        )
        .await;
        let rows = client
            .query(
                "SELECT id,v IS NULL,v::text,seq FROM copy_forced_json_sequence ORDER BY id",
                &[],
            )
            .await
            .expect("read forced JSON and sequence values");
        let next = client
            .query(
                "INSERT INTO copy_forced_json_sequence(id) VALUES (4) RETURNING seq",
                &[],
            )
            .await
            .expect("advance sequence after COPY");
        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert_eq!(copied.0, b'G', "{:?}", copied.1);
        assert_eq!(copied.2, None);
        assert_eq!(copied.3.as_deref(), Some("COPY 2"));
        assert_eq!(omitted.0, b'G');
        assert_eq!(omitted.2, None);
        assert_eq!(omitted.3.as_deref(), Some("COPY 1"));
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert!(!rows[0].get::<_, bool>(1));
        assert_eq!(rows[0].get::<_, Option<String>>(2).as_deref(), Some("null"));
        assert_eq!(rows[0].get::<_, i32>(3), 1);
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert!(rows[1].get::<_, bool>(1));
        assert_eq!(rows[1].get::<_, Option<String>>(2), None);
        assert_eq!(rows[1].get::<_, i32>(3), 2);
        assert_eq!(rows[2].get::<_, i32>(0), 3);
        assert_eq!(
            rows[2].get::<_, Option<String>>(2).as_deref(),
            Some("{\"default\": 1}")
        );
        assert_eq!(rows[2].get::<_, i32>(3), 3);
        assert_eq!(next[0].get::<_, i32>(0), 4);
    });
}

#[test]
fn should_rollback_forced_null_rows_given_a_not_null_constraint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_forced_null_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE copy_forced_required (id INT, v TEXT NOT NULL DEFAULT 'fallback')",
                "INSERT INTO copy_forced_required VALUES (0,'kept')",
            ],
        )
        .await
        .expect("seed committed target");

        // Act
        let (tag, _, state, command) = super::copy_from_stdin(
            &server,
            "COPY copy_forced_required FROM STDIN WITH (FORMAT CSV, NULL 'N', FORCE_NULL(v))",
            b"1,good\n2,\"N\"\n",
        )
        .await;
        let rows = client
            .query("SELECT id,v FROM copy_forced_required ORDER BY id", &[])
            .await
            .expect("read target after rejected NULL");
        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert_eq!(tag, b'G');
        assert_eq!(state.as_deref(), Some("23502"));
        assert_eq!(command, None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 0);
        assert_eq!(rows[0].get::<_, String>(1), "kept");
    });
}
