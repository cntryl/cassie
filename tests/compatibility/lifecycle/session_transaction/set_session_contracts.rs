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
fn should_set_client_encoding_with_set_names() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("set_names_encoding").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("SET NAMES 'LATIN1'")
            .await
            .expect("set the client encoding through SET NAMES");
        let encoding: String = client
            .query_one("SHOW client_encoding", &[])
            .await
            .expect("read the selected client encoding")
            .get(0);

        // Assert
        assert_eq!(encoding, "LATIN1");

        drop(client);
        server.shutdown(connection).await;
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
            .batch_execute(
                "CREATE SCHEMA compat_set_schema; \
                 CREATE TABLE compat_set_schema.values_table (value INT); \
                 INSERT INTO compat_set_schema.values_table VALUES (817)",
            )
            .await
            .expect("create and seed a table in a dedicated schema");

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
