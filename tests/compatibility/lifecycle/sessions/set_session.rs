use super::*;

#[test]
fn should_restore_startup_timezone_with_set_time_zone_default() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("set_timezone_default").await;
        let (client, connection) = server.connect().await;
        let startup_timezone: String = client
            .query_one("SHOW TimeZone", &[])
            .await
            .expect("read the session startup timezone")
            .get(0);
        client
            .batch_execute("SET TIME ZONE 'America/Los_Angeles'")
            .await
            .expect("set a distinct session timezone");

        // Act
        client
            .batch_execute("SET TIME ZONE DEFAULT")
            .await
            .expect("restore the configured default timezone with DEFAULT");
        let restored_timezone: String = client
            .query_one("SHOW TimeZone", &[])
            .await
            .expect("read the timezone restored by DEFAULT")
            .get(0);

        // Assert
        assert_eq!(restored_timezone, startup_timezone);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_local_time_zone_form_to_the_session_default() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("set_timezone_local").await;
        let (client, connection) = server.connect().await;
        let startup_timezone: String = client
            .query_one("SHOW TimeZone", &[])
            .await
            .expect("read the session startup timezone")
            .get(0);
        client
            .batch_execute("SET TIME ZONE 'America/Los_Angeles'")
            .await
            .expect("set a distinct session timezone");

        // Act
        client
            .batch_execute("SET TIME ZONE LOCAL")
            .await
            .expect("apply the server local timezone with LOCAL");
        let local_timezone: String = client
            .query_one("SHOW TimeZone", &[])
            .await
            .expect("read the timezone selected by LOCAL")
            .get(0);

        // Assert
        assert_eq!(local_timezone, startup_timezone);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_interval_time_zone_offset_to_timestamp_text() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("set_timezone_interval").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("SET TIME ZONE INTERVAL '-08:00' HOUR TO MINUTE")
            .await
            .expect("set the session timezone from a typed interval");
        let formatted: String = client
            .query_one("SELECT TIMESTAMPTZ '2024-01-01 00:00:00+00'::text", &[])
            .await
            .expect("format a timestamp using the interval timezone")
            .get(0);

        // Assert
        assert_eq!(formatted, "2023-12-31 16:00:00-08");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_set_session_characteristics_for_the_next_transaction_only() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("set_session_characteristics").await;
        let (writer, writer_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        writer
            .batch_execute("CREATE TABLE compat_session_characteristics (id INT PRIMARY KEY)")
            .await
            .expect("create the read-only transaction fixture");
        let observer_default: String = observer
            .query_one("SHOW default_transaction_read_only", &[])
            .await
            .expect("read the independent session default")
            .get(0);

        // Act
        writer
            .batch_execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY")
            .await
            .expect("set the writer's default transaction characteristics");
        let writer_default: String = writer
            .query_one("SHOW default_transaction_read_only", &[])
            .await
            .expect("read the writer's transaction default")
            .get(0);
        writer
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction with the configured characteristics");
        let transaction_mode: String = writer
            .query_one("SHOW transaction_read_only", &[])
            .await
            .expect("read the active transaction mode")
            .get(0);
        let rejected_write = writer
            .execute("INSERT INTO compat_session_characteristics VALUES (1)", &[])
            .await
            .expect_err("a read-only transaction must reject writes");
        writer
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the read-only transaction");
        writer
            .batch_execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE")
            .await
            .expect("restore writable transaction defaults");
        let rows: i64 = observer
            .query_one("SELECT count(*) FROM compat_session_characteristics", &[])
            .await
            .expect("verify the rejected write left no row")
            .get(0);

        // Assert
        assert_eq!(observer_default, "off");
        assert_eq!(writer_default, "on");
        assert_eq!(transaction_mode, "on");
        assert_eq!(db_error(&rejected_write).code().code(), "25006");
        assert_eq!(rows, 0);

        drop(writer);
        drop(observer);
        server.shutdown(writer_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}

#[test]
fn should_apply_set_schema_for_unqualified_relation_resolution() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("set_schema_resolution").await;
        let (client, connection) = server.connect().await;
        client
            .execute("CREATE SCHEMA compat_set_schema", &[])
            .await
            .expect("create a dedicated schema");
        client
            .execute(
                "CREATE TABLE compat_set_schema.values_table (value INT)",
                &[],
            )
            .await
            .expect("create a table in the dedicated schema");
        client
            .execute(
                "INSERT INTO compat_set_schema.values_table VALUES (817)",
                &[],
            )
            .await
            .expect("seed the schema-qualified relation");

        // Act
        client
            .batch_execute("SET SCHEMA 'compat_set_schema'")
            .await
            .expect("set the session schema");
        let search_path: String = client
            .query_one("SHOW search_path", &[])
            .await
            .expect("read the schema selected by SET SCHEMA")
            .get(0);
        let current_schema: String = client
            .query_one("SELECT current_schema()", &[])
            .await
            .expect("read the current schema")
            .get(0);
        let unqualified_value: i32 = client
            .query_one("SELECT value FROM values_table", &[])
            .await
            .expect("resolve a relation through the selected schema")
            .get(0);

        // Assert
        assert_eq!(search_path, "compat_set_schema");
        assert_eq!(current_schema, "compat_set_schema");
        assert_eq!(unqualified_value, 817);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_accept_postgresql_dump_session_preamble_settings() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pg_dump_session_preamble").await;
        let (client, connection) = server.connect().await;
        let settings = [
            ("statement_timeout", "SET statement_timeout = '0'", "0"),
            ("lock_timeout", "SET lock_timeout = '0'", "0"),
            (
                "idle_in_transaction_session_timeout",
                "SET idle_in_transaction_session_timeout = '0'",
                "0",
            ),
            (
                "check_function_bodies",
                "SET check_function_bodies = off",
                "off",
            ),
            ("xmloption", "SET xmloption = content", "content"),
            ("row_security", "SET row_security = off", "off"),
            (
                "default_table_access_method",
                "SET default_table_access_method = heap",
                "heap",
            ),
            (
                "client_min_messages",
                "SET client_min_messages = warning",
                "warning",
            ),
            (
                "standard_conforming_strings",
                "SET standard_conforming_strings = on",
                "on",
            ),
        ];

        // Act
        let mut failures = Vec::new();
        for (name, set_sql, expected) in settings {
            if let Err(error) = client.batch_execute(set_sql).await {
                failures.push(format!(
                    "{name}: SET failed with SQLSTATE {}",
                    db_error(&error).code().code()
                ));
                continue;
            }
            match client.query_one(&format!("SHOW {name}"), &[]).await {
                Ok(row) => {
                    let actual: String = row.get(0);
                    if actual != expected {
                        failures.push(format!("{name}: expected {expected:?}, got {actual:?}"));
                    }
                }
                Err(error) => failures.push(format!(
                    "{name}: SHOW failed with SQLSTATE {}",
                    db_error(&error).code().code()
                )),
            }
        }

        // Assert
        assert!(
            failures.is_empty(),
            "PostgreSQL dump preamble settings did not match PostgreSQL 18.6: {failures:?}"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_statement_timeout_from_pgwire_startup_options() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("startup_statement_timeout_option").await;
        let mut config = tokio_postgres::Config::new();
        config
            .host("127.0.0.1")
            .port(server.addr.port())
            .user("root")
            .password("postgres")
            .dbname("postgres")
            .options("-c statement_timeout=1234ms");
        let (client, connection) = config
            .connect(NoTls)
            .await
            .expect("connect with a PostgreSQL startup option");
        let connection = tokio::spawn(async move {
            connection
                .await
                .expect("startup-options connection should stay healthy");
        });

        // Act
        let statement_timeout: String = client
            .query_one("SHOW statement_timeout", &[])
            .await
            .expect("read the startup-configured statement timeout")
            .try_get(0)
            .expect("statement_timeout text");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(statement_timeout, "1234ms");
    });
}

#[test]
fn should_reset_all_session_settings_to_connection_startup_defaults() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reset_all_session_defaults").await;
        let (client, connection) = server.connect().await;
        let startup_application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read the application name at connection startup")
            .get(0);
        let startup_search_path: String = client
            .query_one("SHOW search_path", &[])
            .await
            .expect("read the search path at connection startup")
            .get(0);
        client
            .batch_execute(
                "SET application_name = 'compat_reset_all_override'; \
                 SET search_path TO pg_catalog, public",
            )
            .await
            .expect("change session settings before RESET ALL");

        // Act
        let reset = client.batch_execute("RESET ALL").await;
        let settings_after_reset = if reset.is_ok() {
            let application_name: String = client
                .query_one("SHOW application_name", &[])
                .await
                .expect("read application_name after RESET ALL")
                .get(0);
            let search_path: String = client
                .query_one("SHOW search_path", &[])
                .await
                .expect("read search_path after RESET ALL")
                .get(0);
            Some((application_name, search_path))
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        reset.expect("RESET ALL should restore connection-startup settings");
        let (application_name, search_path) =
            settings_after_reset.expect("read both settings after RESET ALL succeeds");
        assert_eq!(application_name, startup_application_name);
        assert_eq!(search_path, startup_search_path);
    });
}

#[test]
fn should_restore_session_override_after_reset_all_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reset_all_rollback").await;
        let (client, connection) = server.connect().await;
        let startup_application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read application_name at connection startup")
            .get(0);
        client
            .batch_execute("SET application_name = 'compat_821_outer'")
            .await
            .expect("set the session value that should survive rollback");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction before resetting session settings");
        client
            .batch_execute("SET application_name = 'compat_821_inner'")
            .await
            .expect("set a transaction-local override before RESET ALL");

        // Act
        client
            .batch_execute("RESET ALL")
            .await
            .expect("reset all settings inside the transaction");
        let after_reset: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read the startup default after RESET ALL")
            .get(0);
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction containing RESET ALL");
        let after_rollback: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read the session value restored by rollback")
            .get(0);

        // Assert
        assert_eq!(after_reset, startup_application_name);
        assert_eq!(after_rollback, "compat_821_outer");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_discard_all_inside_transaction_without_clearing_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_all_transaction_rejection").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("SET application_name = 'compat_821_discard'")
            .await
            .expect("set session state before the transaction-block rejection");
        let prepared = client
            .prepare("SELECT 'retained'::TEXT")
            .await
            .expect("prepare a protocol statement before DISCARD ALL");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction before DISCARD ALL");

        // Act
        let discard = client.batch_execute("DISCARD ALL").await;
        let sqlstate = discard
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("recover the session after DISCARD ALL is rejected");
        assert_eq!(sqlstate.as_deref(), Some("25001"));

        let application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read the retained session setting after recovery")
            .get(0);
        let prepared_result: String = client
            .query_one(&prepared, &[])
            .await
            .expect("execute the prepared statement retained by the failed DISCARD ALL")
            .get(0);

        // Assert
        assert_eq!(application_name, "compat_821_discard");
        assert_eq!(prepared_result, "retained");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_connection_startup_value_given_setting_override_when_reset_or_set_default_runs() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reset_startup_application_name").await;
        let mut config = tokio_postgres::Config::new();
        config
            .host("127.0.0.1")
            .port(server.addr.port())
            .user("root")
            .password("postgres")
            .dbname("postgres")
            .application_name("startup-reset-witness");
        let (client, connection) = config
            .connect(NoTls)
            .await
            .expect("connect with a startup setting");
        let connection = tokio::spawn(async move {
            connection.await.expect("connection remains healthy");
        });
        client
            .batch_execute("SET application_name = 'override'")
            .await
            .expect("override startup value");

        // Act
        client
            .batch_execute("RESET application_name")
            .await
            .expect("reset one setting");
        let reset: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read single reset")
            .get(0);
        client
            .batch_execute("SET application_name TO 'DEFAULT'")
            .await
            .expect("quoted DEFAULT is a literal");
        let literal: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read literal")
            .get(0);
        client
            .batch_execute("SET SESSION application_name TO DEFAULT")
            .await
            .expect("apply typed default operation");
        let default: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read reset default")
            .get(0);
        client
            .batch_execute("SET application_name = 'another'; RESET ALL")
            .await
            .expect("reset all settings");
        let all: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read all-reset default")
            .get(0);

        // Assert
        assert_eq!(reset, "startup-reset-witness");
        assert_eq!(literal, "DEFAULT");
        assert_eq!(default, "startup-reset-witness");
        assert_eq!(all, "startup-reset-witness");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_current_transaction_mode_given_reset_all_when_read_only_transaction_is_active() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reset_all_read_only_mode").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_reset_read_only (id INT PRIMARY KEY)")
            .await
            .expect("create ordinary table");
        client
            .batch_execute("BEGIN READ ONLY")
            .await
            .expect("begin read-only transaction");
        client
            .query_one("SELECT 1", &[])
            .await
            .expect("establish query boundary");

        // Act
        client
            .batch_execute("RESET ALL")
            .await
            .expect("reset configuration without altering transaction mode");
        let mode: String = client
            .query_one("SHOW transaction_read_only", &[])
            .await
            .expect("read transaction mode")
            .get(0);
        let write = client
            .execute("INSERT INTO compat_reset_read_only VALUES (1)", &[])
            .await
            .expect_err("reset cannot enable ordinary table writes");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("recover transaction");

        // Assert
        assert_eq!(mode, "on");
        assert_eq!(db_error(&write).code().code(), "25006");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_read_write_mode_given_prior_query_when_read_only_transaction_changes_mode() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("read_only_mode_query_boundary").await;
        let (client, connection) = server.connect().await;
        client.batch_execute("BEGIN READ ONLY; SET application_name = 'settings-are-allowed'; SET transaction_read_only=off").await.expect("settings do not establish a query boundary");
        client.query_one("SELECT 1", &[]).await.expect("run first query");
        client.batch_execute("SET transaction_read_only=on").await.expect("switching to read-only remains permitted");

        // Act
        let rejected = client.batch_execute("SET transaction_read_only=off").await.expect_err("read-write must be selected before the first query");
        let aborted = client.query_one("SELECT 1", &[]).await.expect_err("mode error aborts the transaction");
        client.batch_execute("ROLLBACK").await.expect("recover failed transaction");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "25001");
        assert_eq!(db_error(&aborted).code().code(), "25P02");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_ordinary_table_mutations_given_read_only_transaction_when_commands_execute() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("read_only_ddl_copy_admission").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_read_only_copy (id INT PRIMARY KEY)")
            .await
            .expect("create ordinary table");

        // Act
        client
            .batch_execute("BEGIN READ ONLY")
            .await
            .expect("begin read-only DDL transaction");
        let ddl = client
            .batch_execute("CREATE TABLE compat_denied_read_only_ddl (id INT)")
            .await
            .expect_err("ordinary DDL is forbidden");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("recover DDL rejection");
        client
            .batch_execute("BEGIN READ ONLY")
            .await
            .expect("begin read-only COPY transaction");
        let copy = client
            .batch_execute("COPY compat_read_only_copy FROM STDIN WITH (FORMAT csv)")
            .await
            .expect_err("COPY must reject before CopyInResponse");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("recover COPY rejection");
        let rows: i64 = client
            .query_one("SELECT count(*) FROM compat_read_only_copy", &[])
            .await
            .expect("read unchanged table")
            .get(0);
        client
            .batch_execute("CREATE TABLE compat_denied_read_only_ddl (id INT)")
            .await
            .expect("rejected DDL left no catalog object");

        // Assert
        assert_eq!(db_error(&ddl).code().code(), "25006");
        assert_eq!(db_error(&copy).code().code(), "25006");
        assert_eq!(rows, 0);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_transaction_modes_given_postgresql_boolean_prefixes_when_settings_change() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transaction_guc_boolean_prefixes").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_boolean_prefix_modes (id INT PRIMARY KEY)")
            .await
            .expect("create ordinary table");

        // Act
        client
            .batch_execute("SET default_transaction_read_only='t'")
            .await
            .expect("accept unambiguous true prefix");
        let default_on: String = client
            .query_one("SHOW default_transaction_read_only", &[])
            .await
            .expect("read canonical true setting")
            .get(0);
        let default_write = client
            .execute("INSERT INTO compat_boolean_prefix_modes VALUES (1)", &[])
            .await
            .expect_err("true prefix must enforce read-only default");
        client
            .batch_execute("SET default_transaction_read_only='of'")
            .await
            .expect("accept unambiguous off prefix");
        client
            .execute("INSERT INTO compat_boolean_prefix_modes VALUES (2)", &[])
            .await
            .expect("off prefix restores writable transactions");
        client
            .batch_execute("BEGIN READ WRITE; SET transaction_read_only='y'")
            .await
            .expect("accept yes prefix for active transaction mode");
        let mode_on: String = client
            .query_one("SHOW transaction_read_only", &[])
            .await
            .expect("read canonical transaction mode")
            .get(0);
        let transaction_write = client
            .execute("INSERT INTO compat_boolean_prefix_modes VALUES (3)", &[])
            .await
            .expect_err("yes prefix must enforce read-only mode");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("recover rejected write");
        let count: i64 = client
            .query_one("SELECT count(*) FROM compat_boolean_prefix_modes", &[])
            .await
            .expect("read only the admitted write")
            .get(0);

        // Assert
        assert_eq!(default_on, "on");
        assert_eq!(mode_on, "on");
        assert_eq!(db_error(&default_write).code().code(), "25006");
        assert_eq!(db_error(&transaction_write).code().code(), "25006");
        assert_eq!(count, 1);
        drop(client);
        server.shutdown(connection).await;
    });
}
