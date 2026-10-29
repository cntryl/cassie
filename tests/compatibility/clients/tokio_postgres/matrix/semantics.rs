fn psql_probe_command(psql_bin: &str, connection: &str) -> Command {
    let mut command = Command::new(psql_bin);
    command.args([
        "-X",
        "--tuples-only",
        "--no-align",
        "-v",
        "ON_ERROR_STOP=1",
        "-v",
        "VERBOSITY=sqlstate",
        "-d",
        connection,
        "-c",
        "CREATE TABLE compat_psql_probe (id INT PRIMARY KEY, title TEXT UNIQUE NOT NULL)",
        "-c",
        "CREATE TABLE compat_psql_status (id INT PRIMARY KEY, status TEXT NOT NULL)",
        "-c",
        "BEGIN",
        "-c",
        "INSERT INTO compat_psql_probe VALUES (1, 'alpha')",
        "-c",
        "INSERT INTO compat_psql_status VALUES (1, 'active')",
        "-c",
        "COMMIT",
        "-c",
        "SELECT title FROM compat_psql_probe ORDER BY title",
        "-c",
        "SELECT 'psql_catalog', table_name FROM information_schema.tables \
                 WHERE table_name = 'compat_psql_probe'",
        "-c",
        "SELECT 'psql_join', p.title, s.status FROM compat_psql_probe p \
                 JOIN compat_psql_status s ON p.id = s.id WHERE p.id = 1",
        "-c",
        "SELECT 'psql_after_commit', COUNT(*) FROM compat_psql_probe",
        "-c",
        "BEGIN",
        "-c",
        "INSERT INTO compat_psql_probe VALUES (2, 'beta')",
        "-c",
        "ROLLBACK",
        "-c",
        "SELECT 'psql_after_rollback', COUNT(*) FROM compat_psql_probe",
        "-c",
        "SELECT 'psql_pid_before', pg_backend_pid()",
        "-c",
        "\\set ON_ERROR_STOP off",
        "-c",
        "INSERT INTO compat_psql_probe VALUES (3, 'alpha')",
        "-c",
        "\\set ON_ERROR_STOP on",
        "-c",
        "SELECT 'psql_after_duplicate', title FROM compat_psql_probe WHERE id = 1",
        "-c",
        "SELECT 'psql_pid_after_duplicate', pg_backend_pid()",
        "-c",
        "\\set ON_ERROR_STOP off",
        "-c",
        "SELECT title FROM compat_psql_missing",
        "-c",
        "\\set ON_ERROR_STOP on",
        "-c",
        "SELECT 'psql_after_missing', title FROM compat_psql_probe WHERE id = 1",
        "-c",
        "SELECT 'psql_pid_after_missing', pg_backend_pid()",
    ]);
    command
}

fn assert_psql_probe(output: &ProbeOutput) {
    assert!(
        !output.timed_out,
        "psql timed out\nstdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
    assert!(
        output.success,
        "psql failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status_code, output.stdout, output.stderr
    );
    assert!(output.stdout.lines().any(|line| line.trim() == "alpha"));
    for expected in [
        "psql_catalog|compat_psql_probe",
        "psql_join|alpha|active",
        "psql_after_commit|1",
        "psql_after_rollback|1",
        "psql_after_duplicate|alpha",
        "psql_after_missing|alpha",
    ] {
        assert!(
            output.stdout.lines().any(|line| line.trim() == expected),
            "missing expected psql marker: {expected}\nstdout:\n{}\nstderr:\n{}",
            output.stdout,
            output.stderr
        );
    }
    let pid = |prefix: &str| {
        output
            .stdout
            .lines()
            .find_map(|line| line.trim().strip_prefix(prefix))
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value != 0)
            .unwrap_or_else(|| panic!("missing nonzero psql backend PID: {prefix}"))
    };
    let initial_pid = pid("psql_pid_before|");
    assert_eq!(pid("psql_pid_after_duplicate|"), initial_pid);
    assert_eq!(pid("psql_pid_after_missing|"), initial_pid);
    let errors: Vec<_> = output
        .stderr
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("ERROR:"))
        .collect();
    assert_eq!(errors, ["ERROR:  23505", "ERROR:  42P01"]);
    for marker in [
        "psql_catalog|compat_psql_probe",
        "psql_join|alpha|active",
        "psql_after_commit|1",
        "psql_after_rollback|1",
        "psql_after_duplicate|alpha",
        "psql_after_missing|alpha",
        "psql_error_duplicate|23505",
        "psql_error_missing|42P01",
        "psql_same_session|true",
    ] {
        println!("{marker}");
    }
}

#[test]
#[ignore = "requires local psql; run with CASSIE_RUN_PSQL_COMPAT=1 cargo test --locked --test compatibility compatibility_matrix::should_validate_psql_read_model_probe_when_enabled -- --ignored --nocapture"]
fn should_validate_psql_read_model_probe_when_enabled() {
    // Arrange
    if std::env::var("CASSIE_RUN_PSQL_COMPAT").ok().as_deref() != Some("1") {
        eprintln!("set CASSIE_RUN_PSQL_COMPAT=1 to run the optional psql probe");
        return;
    }
    let psql_bin = std::env::var("CASSIE_PSQL_BIN").unwrap_or_else(|_| "psql".to_string());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("psql_probe").await;
        let connection = format!(
            "postgresql://root:postgres@127.0.0.1:{}/postgres",
            server.addr.port()
        );

        // Act
        let output = tokio::task::spawn_blocking(move || {
            let command = psql_probe_command(&psql_bin, &connection);
            run_external_probe(command, Duration::from_secs(20))
        })
        .await
        .expect("psql probe blocking task should complete");
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                server.shutdown_without_client().await;
                panic!("run psql compatibility probe: {error}");
            }
        };
        server.shutdown_without_client().await;

        // Assert
        assert_psql_probe(&output);
    });
}

#[test]
#[ignore = "requires local Prisma CLI; run with CASSIE_RUN_PRISMA_COMPAT=1 cargo test --locked --test compatibility compatibility_matrix::should_validate_prisma_introspection_probe_when_enabled -- --ignored --nocapture"]
fn should_validate_prisma_introspection_probe_when_enabled() {
    // Arrange
    if std::env::var("CASSIE_RUN_PRISMA_COMPAT").ok().as_deref() != Some("1") {
        eprintln!("set CASSIE_RUN_PRISMA_COMPAT=1 to run the optional Prisma probe");
        return;
    }
    let prisma_bin = std::env::var("CASSIE_PRISMA_BIN").unwrap_or_else(|_| "prisma".to_string());
    let schema_dir = temp_dir("prisma_probe");
    fs::create_dir_all(&schema_dir).expect("create Prisma probe directory");
    let schema_path = schema_dir.join("schema.prisma");
    fs::write(
        &schema_path,
        r#"generator client {
  provider = "prisma-client-js"
}

datasource db {
  provider = "postgresql"
  url      = env("DATABASE_URL")
}
"#,
    )
    .expect("write Prisma schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("prisma_probe").await;
        let (client, connection_task) =
            tokio::time::timeout(Duration::from_secs(5), server.connect())
                .await
                .expect("connect should complete within the timeout");
        client
            .batch_execute(
                "CREATE TABLE compat_prisma_items (id INT PRIMARY KEY, title TEXT NOT NULL UNIQUE, created_at TIMESTAMP)",
            )
            .await
            .expect("Prisma probe fixture table should be created");
        drop(client);
        connection_task.abort();
        let _ = connection_task.await;

        let connection = format!(
            "postgresql://postgres:postgres@127.0.0.1:{}/postgres",
            server.addr.port()
        );
        let schema_arg = schema_path
            .to_str()
            .expect("Prisma schema path should be UTF-8")
            .to_string();

        // Act
        let output = tokio::task::spawn_blocking(move || {
            let mut command = Command::new(prisma_bin);
            command
                .current_dir(&schema_dir)
                .env("DATABASE_URL", &connection)
                .args(["db", "pull", "--schema", &schema_arg, "--url", &connection, "--print"]);
            let output = run_external_probe(command, Duration::from_secs(45));
            let _ = fs::remove_dir_all(&schema_dir);
            output
        })
        .await
        .expect("Prisma probe blocking task should complete");
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                server.shutdown_without_client().await;
                panic!("run Prisma compatibility probe: {error}");
            }
        };
        server.shutdown_without_client().await;

        // Assert
        assert!(
            !output.timed_out,
            "Prisma probe timed out\nstdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
        assert!(
            output.success,
            "Prisma probe failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status_code, output.stdout, output.stderr
        );
        assert!(output.stdout.contains("compat_prisma_items"));
        assert!(output.stdout.contains("title"));
    });
}

#[test]
#[ignore = "requires isolated Node v24.21.0 and Prisma 7.10.0 toolchain; setup and invocation are documented in this test's helper"]
fn should_execute_prisma_7_generated_queries_against_cassie() {
    // Arrange
    // The helper provisions a per-run schema and verifies the pinned external packages.

    // Act
    prisma7_generated_query::run_prisma_7_generated_queries_against_cassie();

    // Assert
    // The helper asserts generated SQL, bind values, pagination, raw results and delegate values.
}

#[test]
fn should_preserve_session_search_path_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("session_search_path").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");
        client
            .batch_execute("CREATE SCHEMA reporting")
            .await
            .expect("schema creation should succeed");
        client
            .batch_execute("CREATE TABLE public.visible_docs (title TEXT)")
            .await
            .expect("public table creation should succeed");
        client
            .batch_execute("CREATE TABLE reporting.visible_docs (title TEXT)")
            .await
            .expect("reporting table creation should succeed");
        client
            .batch_execute("INSERT INTO public.visible_docs (title) VALUES ('public-doc')")
            .await
            .expect("public row insert should succeed");
        client
            .batch_execute("INSERT INTO reporting.visible_docs (title) VALUES ('reporting-doc')")
            .await
            .expect("reporting row insert should succeed");
        let initial_path = first_simple_value(
            client
                .simple_query("SHOW search_path")
                .await
                .expect("initial search_path should be readable"),
        );

        // Act
        client
            .batch_execute("SET search_path = reporting")
            .await
            .expect("search_path update should succeed");
        let updated_path = first_simple_value(
            client
                .simple_query("SHOW search_path")
                .await
                .expect("updated search_path should be readable"),
        );
        let schema = client
            .query_one("SELECT current_schema()", &[])
            .await
            .expect("current schema should be readable");
        let rows = client
            .query("SELECT title FROM visible_docs ORDER BY title", &[])
            .await
            .expect("unqualified select should resolve through search_path");

        // Assert
        assert_eq!(initial_path, "public");
        assert_eq!(updated_path, "reporting");
        let current_schema: String = schema.try_get(0).expect("current_schema column");
        assert_eq!(current_schema, "reporting");
        assert_eq!(rows.len(), 1);
        let title: String = rows[0].try_get(0).expect("title column");
        assert_eq!(title, "reporting-doc");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_accept_pgadmin_date_style_session_setup() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("pgadmin_date_style").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("SET DateStyle = 'ISO, MDY'")
            .await
            .expect("pgAdmin DateStyle setup should succeed");
        let value = first_simple_value(
            client
                .simple_query("SHOW DateStyle")
                .await
                .expect("DateStyle should be readable"),
        );

        // Assert
        assert_eq!(value, "ISO, MDY");
        server.shutdown(connection).await;
    });
}

#[test]
fn should_accept_pgadmin_client_min_messages_session_setup() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("pgadmin_client_min_messages").await;
        let (client, connection) = server.connect().await;

        // Act
        let result = client
            .batch_execute("SET client_min_messages = 'warning'")
            .await;

        // Assert
        result.expect("pgAdmin client_min_messages setup should succeed");
        server.shutdown(connection).await;
    });
}

#[test]
fn should_accept_missing_search_path_schema_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("missing_search_path_schema").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        let set = client
            .batch_execute("SET search_path = missing_schema, public")
            .await;

        // Assert
        assert!(
            set.is_ok(),
            "PostgreSQL ignores missing search_path schemas"
        );
        let shown = client
            .simple_query("SHOW search_path")
            .await
            .expect("show search_path")
            .into_iter()
            .find_map(|message| match message {
                tokio_postgres::SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
                _ => None,
            });
        assert_eq!(shown.as_deref(), Some("missing_schema, public"));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_read_show_search_path_through_prepared_query_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_show_search_path").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");
        client
            .batch_execute("SET search_path = missing_schema, public")
            .await
            .expect("set search_path");

        // Act
        let row = tokio::time::timeout(
            Duration::from_secs(5),
            client.query_one("SHOW search_path", &[]),
        )
        .await
        .expect("prepared SHOW should complete within the timeout");

        // Assert
        let row = row.expect("prepared SHOW should decode as one row");
        assert_eq!(row.columns()[0].name(), "search_path");
        let shown: String = row.try_get(0).expect("search_path column");
        assert_eq!(shown, "missing_schema, public");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_name_prepared_show_columns_like_postgres_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_show_columns").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        let mut named = Vec::new();
        for sql in ["SHOW TRANSACTION ISOLATION LEVEL", "SHOW DateStyle"] {
            let row = client
                .query_one(sql, &[])
                .await
                .unwrap_or_else(|_| panic!("{sql} should decode as one row"));
            let value: String = row.try_get(0).expect("setting value");
            named.push((row.columns()[0].name().to_string(), value.is_empty()));
        }

        // Assert
        assert_eq!(
            named,
            vec![
                ("transaction_isolation".to_string(), false),
                ("DateStyle".to_string(), false),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_query_prepared_statement_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_query").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        let row = tokio::time::timeout(
            Duration::from_secs(5),
            client.query_one("SELECT version()", &[]),
        )
        .await
        .expect("prepared query should complete within the timeout")
        .expect("query row");

        // Assert
        let version: String = row.try_get(0).expect("version column");
        assert!(version.starts_with("PostgreSQL 16.0 compatible Cassie "));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_round_trip_ddl_dml_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("ddl_dml_round_trip").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        client
            .batch_execute("CREATE SCHEMA compat_pgwire_round_trip")
            .await
            .expect("schema creation should succeed");
        client
            .batch_execute("CREATE TABLE compat_pgwire_round_trip_items (title TEXT)")
            .await
            .expect("table creation should succeed");

        let inserted = client
            .execute(
                "INSERT INTO compat_pgwire_round_trip_items (title) VALUES ($1)",
                &[&"alpha"],
            )
            .await
            .expect("insert should succeed");
        let rows = client
            .query(
                "SELECT title FROM compat_pgwire_round_trip_items ORDER BY title",
                &[],
            )
            .await
            .expect("select should succeed");

        // Assert
        assert_eq!(inserted, 1);
        assert_eq!(rows.len(), 1);
        let title: String = rows[0].try_get(0).expect("title column");
        assert_eq!(title, "alpha");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_create_call_user_defined_function_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("udf_call").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        client
            .batch_execute(r#"CREATE FUNCTION compat_echo(x INT) RETURNS INT AS "x""#)
            .await
            .expect("function creation should succeed");
        let statement = client
            .prepare("SELECT compat_echo($1)")
            .await
            .expect("function input should be described");
        let row = client
            .query_one(&statement, &[&7_i32])
            .await
            .expect("function call should succeed");

        // Assert
        assert_eq!(statement.params(), [tokio_postgres::types::Type::INT4]);
        let echoed: i32 = row.try_get(0).expect("function return value");
        assert_eq!(echoed, 7);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_create_call_procedure_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("procedure_call").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        client
            .batch_execute(
                "CREATE TABLE compat_procedure_calls (title TEXT)",
            )
            .await
            .expect("table creation should succeed");
        client
            .batch_execute(
                r#"CREATE PROCEDURE compat_store_title(title TEXT) AS "INSERT INTO compat_procedure_calls (title) VALUES ($1)""#,
            )
            .await
            .expect("procedure creation should succeed");
        let affected = client
            .execute("CALL compat_store_title($1)", &[&"alpha"])
            .await
            .expect("procedure call should succeed");
        let rows = client
            .query("SELECT title FROM compat_procedure_calls ORDER BY title", &[])
            .await
            .expect("select should succeed");

        // Assert
        assert_eq!(affected, 0);
        assert_eq!(rows.len(), 1);
        let title: String = rows[0].try_get(0).expect("title column");
        assert_eq!(title, "alpha");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_run_on_conflict_upsert_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("on_conflict_upsert").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        client
            .batch_execute("CREATE TABLE compat_upsert_items (id INT PRIMARY KEY, title TEXT)")
            .await
            .expect("table creation should succeed");
        client
            .execute(
                "INSERT INTO compat_upsert_items (id, title) VALUES ($1, $2)",
                &[&1_i32, &"alpha"],
            )
            .await
            .expect("initial insert should succeed");
        let updated = client
            .query_one(
                "INSERT INTO compat_upsert_items (id, title) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET title = excluded.title RETURNING title",
                &[&1_i32, &"beta"],
            )
            .await
            .expect("upsert should succeed");

        // Assert
        let title: String = updated.try_get(0).expect("title column");
        assert_eq!(title, "beta");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_foreign_keys_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("foreign_keys").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        client
            .batch_execute("CREATE TABLE compat_fk_parents (id INT PRIMARY KEY, title TEXT)")
            .await
            .expect("parent table creation should succeed");
        client
            .batch_execute(
                "CREATE TABLE compat_fk_children (parent_id INT REFERENCES compat_fk_parents(id), title TEXT)",
            )
            .await
            .expect("child table creation should succeed");
        client
            .execute(
                "INSERT INTO compat_fk_parents (id, title) VALUES ($1, $2)",
                &[&1_i32, &"alpha"],
            )
            .await
            .expect("parent insert should succeed");
        client
            .execute(
                "INSERT INTO compat_fk_children (parent_id, title) VALUES ($1, $2)",
                &[&1_i32, &"child"],
            )
            .await
            .expect("child insert should succeed");
        let missing_parent = client
            .execute(
                "INSERT INTO compat_fk_children (parent_id, title) VALUES ($1, $2)",
                &[&2_i32, &"missing"],
            )
            .await;

        // Assert
        let missing_parent = missing_parent.expect_err("missing parent should be rejected");
        let db_error = db_error(&missing_parent);
        assert_eq!(db_error.code().code(), "23503");
        assert_eq!(db_error.table(), Some("compat_fk_children"));
        assert_eq!(db_error.column(), Some("parent_id"));
        assert_eq!(
            db_error.constraint(),
            Some("compat_fk_children_parent_id_foreign_key")
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_run_recursive_cte_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("recursive_cte").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        client
            .batch_execute("CREATE TABLE compat_recursive_seed (n INT)")
            .await
            .expect("seed table should be created");
        client
            .execute("INSERT INTO compat_recursive_seed (n) VALUES ($1)", &[&1_i32])
            .await
            .expect("seed row should be inserted");
        let rows = client
            .query(
                "WITH RECURSIVE seq(n) AS (SELECT n FROM compat_recursive_seed WHERE n = 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 2) SELECT n FROM seq ORDER BY n",
                &[],
            )
            .await
            .expect("recursive cte should succeed");

        // Assert
        // The CTE column keeps the seed table's INT type, so a strictly typed
        // client decodes it as an integer. It previously arrived as text,
        // which is what this assertion used to read.
        let values = rows
            .into_iter()
            .map(|row| row.try_get::<_, i32>(0).expect("cte value"))
            .collect::<Vec<_>>();
        assert_eq!(values, vec![1_i32, 2_i32]);

        drop(client);
        server.shutdown(connection).await;
    });
}
