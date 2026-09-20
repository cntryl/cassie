// Consolidated integration suite: compatibility.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/compatibility_matrix.rs.
mod compatibility_matrix {
    use std::net::SocketAddr;
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};
    use std::{fs, path::PathBuf};

    use super::support_sql as support;
    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use support::*;
    use tokio_postgres::{error::DbError, NoTls, SimpleQueryMessage};
    use uuid::Uuid;

    fn temp_dir(label: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("cassie-compatibility-{}-{}", label, Uuid::new_v4()));
        path
    }

    struct CompatibilityServer {
        data_dir: String,
        addr: SocketAddr,
        server: tokio::task::JoinHandle<()>,
    }

    impl CompatibilityServer {
        async fn start(label: &str) -> Self {
            use_local_storage();
            let data_dir = data_dir(label);
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config.clone()).unwrap();
            cassie.startup().unwrap();

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);

            let server = tokio::spawn(async move {
                let _ =
                    cassie::pgwire::server::run(addr.to_string(), Arc::new(cassie.clone()), config)
                        .await;
            });
            tokio::time::sleep(Duration::from_millis(50)).await;

            Self {
                data_dir,
                addr,
                server,
            }
        }

        async fn connect(&self) -> (tokio_postgres::Client, tokio::task::JoinHandle<()>) {
            let mut config = tokio_postgres::Config::new();
            config.host("127.0.0.1");
            config.port(self.addr.port());
            config.user("root");
            config.password("postgres");
            config.dbname("postgres");

            let (client, connection) = config.connect(NoTls).await.expect("connect tokio-postgres");
            let connection = tokio::spawn(async move {
                connection
                    .await
                    .expect("tokio-postgres connection task should stay healthy");
            });

            (client, connection)
        }

        async fn shutdown(self, connection: tokio::task::JoinHandle<()>) {
            connection.abort();
            self.server.abort();
            let _ = connection.await;
            let _ = self.server.await;
            let _ = std::fs::remove_dir_all(self.data_dir);
        }

        async fn shutdown_without_client(self) {
            self.server.abort();
            let _ = self.server.await;
            let _ = std::fs::remove_dir_all(self.data_dir);
        }
    }

    fn db_error(error: &tokio_postgres::Error) -> &DbError {
        error
            .as_db_error()
            .expect("tokio-postgres should return a database error")
    }

    fn first_simple_value(messages: Vec<SimpleQueryMessage>) -> String {
        messages
            .into_iter()
            .find_map(|message| match message {
                SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
                _ => None,
            })
            .expect("simple query should return a row")
    }

    struct ProbeOutput {
        success: bool,
        timed_out: bool,
        status_code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    fn run_external_probe(mut command: Command, timeout: Duration) -> Result<ProbeOutput, String> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| format!("spawn external probe: {error}"))?;
        let started = Instant::now();

        loop {
            if child
                .try_wait()
                .map_err(|error| format!("poll external probe: {error}"))?
                .is_some()
            {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("collect external probe output: {error}"))?;
                return Ok(ProbeOutput {
                    success: output.status.success(),
                    timed_out: false,
                    status_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }

            if started.elapsed() >= timeout {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("collect timed-out external probe output: {error}"))?;
                return Ok(ProbeOutput {
                    success: false,
                    timed_out: true,
                    status_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }

            thread::sleep(Duration::from_millis(50));
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
            "postgresql://postgres:postgres@127.0.0.1:{}/postgres",
            server.addr.port()
        );

        // Act
        let output = tokio::task::spawn_blocking(move || {
            let mut command = Command::new(psql_bin);
            command.args([
                "-X",
                "--tuples-only",
                "--no-align",
                "-v",
                "ON_ERROR_STOP=1",
                "-d",
                &connection,
                "-c",
                "CREATE TABLE compat_psql_probe (title TEXT); INSERT INTO compat_psql_probe (title) VALUES ('alpha'); SELECT title FROM compat_psql_probe ORDER BY title;",
            ]);
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
        let returned_alpha = output.stdout.lines().any(|line| line.trim() == "alpha");
        server.shutdown_without_client().await;

        // Assert
        assert!(
            !output.timed_out,
            "psql timed out\nstdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
        assert!(
            output.success,
            "psql failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status_code,
            output.stdout,
            output.stderr
        );
        assert!(returned_alpha);
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
        let prisma_bin =
            std::env::var("CASSIE_PRISMA_BIN").unwrap_or_else(|_| "prisma".to_string());
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
    fn should_read_catalog_metadata_after_connect() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let server = CompatibilityServer::start("connect_metadata").await;
            let (client, connection) =
                tokio::time::timeout(Duration::from_secs(5), server.connect())
                    .await
                    .expect("connect should complete within the timeout");

            // Act
            let messages = tokio::time::timeout(
                Duration::from_secs(5),
                client.simple_query("SELECT version(), current_schema(), current_database()"),
            )
            .await
            .expect("metadata query should complete within the timeout")
            .expect("query metadata");

            // Assert
            let row = messages
                .into_iter()
                .find_map(|message| match message {
                    SimpleQueryMessage::Row(row) => Some(row),
                    _ => None,
                })
                .expect("metadata query should return a row");
            assert!(row
                .get(0)
                .is_some_and(|version| version.starts_with("PostgreSQL 16.0 compatible Cassie ")));
            assert_eq!(row.get(1), Some("public"));
            assert_eq!(row.get(2), Some("postgres"));

            drop(client);
            server.shutdown(connection).await;
        });
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
            let (client, connection) =
                tokio::time::timeout(Duration::from_secs(5), server.connect())
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
                .batch_execute(
                    "INSERT INTO reporting.visible_docs (title) VALUES ('reporting-doc')",
                )
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
    fn should_report_missing_search_path_schema_with_tokio_postgres() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let server = CompatibilityServer::start("missing_search_path_schema").await;
            let (client, connection) =
                tokio::time::timeout(Duration::from_secs(5), server.connect())
                    .await
                    .expect("connect should complete within the timeout");

            // Act
            let error = client
                .batch_execute("SET search_path = missing_schema")
                .await
                .expect_err("missing schema should be rejected");

            // Assert
            let db_error = db_error(&error);
            assert_eq!(db_error.code().code(), "3F000");
            assert!(db_error.message().contains("missing_schema"));

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
            let (client, connection) =
                tokio::time::timeout(Duration::from_secs(5), server.connect())
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
            let (client, connection) =
                tokio::time::timeout(Duration::from_secs(5), server.connect())
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
            let (client, connection) =
                tokio::time::timeout(Duration::from_secs(5), server.connect())
                    .await
                    .expect("connect should complete within the timeout");

            // Act
            client
                .batch_execute(r#"CREATE FUNCTION compat_echo(x INT) RETURNS INT AS "x""#)
                .await
                .expect("function creation should succeed");
            let row = client
                .query_one("SELECT compat_echo($1)", &[&"7"])
                .await
                .expect("function call should succeed");

            // Assert
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
    fn should_report_missing_relation_metadata_with_tokio_postgres() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let server = CompatibilityServer::start("missing_relation").await;
            let (client, connection) =
                tokio::time::timeout(Duration::from_secs(5), server.connect())
                    .await
                    .expect("connect should complete within the timeout");

            // Act
            let error = client
                .query_one("SELECT * FROM compat_missing_relation", &[])
                .await
                .expect_err("missing relation should be rejected");

            // Assert
            let db_error = db_error(&error);
            assert_eq!(db_error.code().code(), "42P01");
            assert_eq!(db_error.table(), Some("compat_missing_relation"));

            drop(client);
            server.shutdown(connection).await;
        });
    }

    #[test]
    fn should_report_unique_violation_metadata_with_tokio_postgres() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let server = CompatibilityServer::start("constraint_metadata").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        client
            .batch_execute(
                "CREATE TABLE compat_constraint_metadata (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE)",
            )
            .await
            .expect("table creation should succeed");
        client
            .execute(
                "INSERT INTO compat_constraint_metadata (id, email) VALUES ($1, $2)",
                &[&1_i32, &"alpha@example.com"],
            )
            .await
            .expect("seed insert should succeed");

        // Act
        let duplicate = client
            .execute(
                "INSERT INTO compat_constraint_metadata (id, email) VALUES ($1, $2)",
                &[&2_i32, &"alpha@example.com"],
            )
            .await
            .expect_err("duplicate unique value should be rejected");

        // Assert
        let duplicate = db_error(&duplicate);
        assert_eq!(duplicate.code().code(), "23505");
        assert_eq!(duplicate.table(), Some("compat_constraint_metadata"));
        assert_eq!(duplicate.column(), Some("email"));
        assert_eq!(
            duplicate.constraint(),
            Some("compat_constraint_metadata_email_unique")
        );

        drop(client);
        server.shutdown(connection).await;
    });
    }

    #[test]
    fn should_report_not_null_violation_metadata_with_tokio_postgres() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let server = CompatibilityServer::start("not_null_metadata").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        client
            .batch_execute(
                "CREATE TABLE compat_not_null_metadata (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE)",
            )
            .await
            .expect("table creation should succeed");

        // Act
        let missing_not_null = client
            .execute(
                "INSERT INTO compat_not_null_metadata (id, email) VALUES ($1, $2)",
                &[&3_i32, &Option::<String>::None],
            )
            .await
            .expect_err("null email should be rejected");

        // Assert
        let missing_not_null = db_error(&missing_not_null);
        assert_eq!(missing_not_null.code().code(), "23502");
        assert_eq!(missing_not_null.table(), Some("compat_not_null_metadata"));
        assert_eq!(missing_not_null.column(), Some("email"));
        assert_eq!(
            missing_not_null.constraint(),
            Some("compat_not_null_metadata_email_not_null")
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

    #[test]
    fn should_recover_after_a_syntax_error_with_tokio_postgres() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let server = CompatibilityServer::start("error_recovery").await;
            let (client, connection) =
                tokio::time::timeout(Duration::from_secs(5), server.connect())
                    .await
                    .expect("connect should complete within the timeout");

            // Act
            let first = client.query_one("SELECT * FROM", &[]).await;
            let second = client
                .query_one("SELECT version()", &[])
                .await
                .expect("connection should recover after a syntax error");

            // Assert
            let first = first.expect_err("syntax error should be rejected");
            let db_error = db_error(&first);
            assert_eq!(db_error.code().code(), "42601");
            let version: String = second.try_get(0).expect("version column");
            assert!(version.starts_with("PostgreSQL 16.0 compatible Cassie "));

            drop(client);
            server.shutdown(connection).await;
        });
    }
}

// Formerly tests/compatibility_sqlalchemy.rs.
mod compatibility_sqlalchemy {
    use std::net::SocketAddr;
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::support_sql as support;
    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use support::*;

    const SQLALCHEMY_PROBE: &str = r#"
import sys

from sqlalchemy import create_engine, text
from sqlalchemy.exc import DBAPIError

url = sys.argv[1]
print("sqlalchemy_step=create_engine", flush=True)
engine = create_engine(url, future=True, use_native_hstore=False)

print("sqlalchemy_step=connect", flush=True)
with engine.connect() as conn:
    print("sqlalchemy_step=ddl", flush=True)
    conn.exec_driver_sql(
        "CREATE TABLE compat_sqlalchemy_probe (id INT PRIMARY KEY, title TEXT NOT NULL UNIQUE)"
    )
    conn.execute(
        text("INSERT INTO compat_sqlalchemy_probe (id, title) VALUES (:id, :title)"),
        {"id": 1, "title": "alpha"},
    )
    conn.commit()

    print("sqlalchemy_step=catalog", flush=True)
    catalog = conn.execute(
        text(
            "SELECT table_name FROM information_schema.tables "
            "WHERE table_name = :table_name"
        ),
        {"table_name": "compat_sqlalchemy_probe"},
    ).scalar_one()
    print("sqlalchemy_step=simple", flush=True)
    simple = conn.exec_driver_sql(
        "SELECT title FROM compat_sqlalchemy_probe ORDER BY title"
    ).scalar_one()
    print("sqlalchemy_step=prepared", flush=True)
    prepared = conn.execute(
        text("SELECT title FROM compat_sqlalchemy_probe WHERE id = :id"),
        {"id": 1},
    ).scalar_one()

    print("sqlalchemy_step=duplicate_error", flush=True)
    try:
        conn.execute(
            text("INSERT INTO compat_sqlalchemy_probe (id, title) VALUES (:id, :title)"),
            {"id": 2, "title": "alpha"},
        )
        conn.commit()
    except DBAPIError as exc:
        conn.rollback()
        duplicate_sqlstate = getattr(getattr(exc, "orig", None), "sqlstate", "")
    else:
        raise AssertionError("duplicate unique insert succeeded")

    print("sqlalchemy_step=missing_relation_error", flush=True)
    try:
        conn.execute(text("SELECT title FROM compat_sqlalchemy_missing")).all()
    except DBAPIError as exc:
        conn.rollback()
        missing_sqlstate = getattr(getattr(exc, "orig", None), "sqlstate", "")
    else:
        raise AssertionError("missing relation query succeeded")

print(f"sqlalchemy_catalog={catalog}")
print(f"sqlalchemy_simple={simple}")
print(f"sqlalchemy_prepared={prepared}")
print(f"sqlalchemy_duplicate_sqlstate={duplicate_sqlstate}")
print(f"sqlalchemy_missing_sqlstate={missing_sqlstate}")
"#;

    struct ProbeOutput {
        success: bool,
        timed_out: bool,
        status_code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    struct CompatibilityServer {
        data_dir: String,
        addr: SocketAddr,
        server: tokio::task::JoinHandle<()>,
    }

    impl CompatibilityServer {
        async fn start(label: &str) -> Self {
            use_local_storage();
            let data_dir = data_dir(label);
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config.clone()).unwrap();
            cassie.startup().unwrap();

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);

            let server = tokio::spawn(async move {
                let _ =
                    cassie::pgwire::server::run(addr.to_string(), Arc::new(cassie.clone()), config)
                        .await;
            });
            tokio::time::sleep(Duration::from_millis(50)).await;

            Self {
                data_dir,
                addr,
                server,
            }
        }

        async fn shutdown_without_client(self) {
            self.server.abort();
            let _ = self.server.await;
            let _ = std::fs::remove_dir_all(self.data_dir);
        }
    }

    fn run_external_probe(mut command: Command, timeout: Duration) -> Result<ProbeOutput, String> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| format!("spawn external probe: {error}"))?;
        let started = Instant::now();

        loop {
            if child
                .try_wait()
                .map_err(|error| format!("poll external probe: {error}"))?
                .is_some()
            {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("collect external probe output: {error}"))?;
                return Ok(ProbeOutput {
                    success: output.status.success(),
                    timed_out: false,
                    status_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }

            if started.elapsed() >= timeout {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("collect timed-out external probe output: {error}"))?;
                return Ok(ProbeOutput {
                    success: false,
                    timed_out: true,
                    status_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }

            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    #[ignore = "requires external SQLAlchemy compatibility harness"]
    fn should_validate_sqlalchemy_read_model_probe_when_enabled() {
        // Arrange
        if std::env::var("CASSIE_RUN_SQLALCHEMY_COMPAT")
            .ok()
            .as_deref()
            != Some("1")
        {
            eprintln!("set CASSIE_RUN_SQLALCHEMY_COMPAT=1 to run the optional SQLAlchemy probe");
            return;
        }
        let python_bin =
            std::env::var("CASSIE_SQLALCHEMY_PYTHON").unwrap_or_else(|_| "python3".to_string());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let server = CompatibilityServer::start("sqlalchemy_probe").await;
            let connection = format!(
                "postgresql+psycopg://postgres:postgres@127.0.0.1:{}/postgres",
                server.addr.port()
            );

            // Act
            let output = tokio::task::spawn_blocking(move || {
                let mut command = Command::new(python_bin);
                command.args(["-c", SQLALCHEMY_PROBE, &connection]);
                run_external_probe(command, Duration::from_secs(20))
            })
            .await
            .expect("SQLAlchemy probe blocking task should complete");
            let output = match output {
                Ok(output) => output,
                Err(error) => {
                    server.shutdown_without_client().await;
                    panic!("run SQLAlchemy compatibility probe: {error}");
                }
            };
            server.shutdown_without_client().await;

            // Assert
            assert!(
                !output.timed_out,
                "SQLAlchemy probe timed out\nstdout:\n{}\nstderr:\n{}",
                output.stdout, output.stderr
            );
            assert!(
                output.success,
                "SQLAlchemy probe failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
                output.status_code, output.stdout, output.stderr
            );
            assert!(output
                .stdout
                .contains("sqlalchemy_catalog=compat_sqlalchemy_probe"));
            assert!(output.stdout.contains("sqlalchemy_simple=alpha"));
            assert!(output.stdout.contains("sqlalchemy_prepared=alpha"));
            assert!(output
                .stdout
                .contains("sqlalchemy_duplicate_sqlstate=23505"));
            assert!(output.stdout.contains("sqlalchemy_missing_sqlstate=42P01"));
        });
    }
}

// Formerly tests/compatibility_diesel.rs.
mod compatibility_diesel {
    use super::support_sql as support;

    use std::net::SocketAddr;
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use support::data_dir;

    struct ProbeOutput {
        success: bool,
        timed_out: bool,
        status_code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    struct CompatibilityServer {
        data_dir: String,
        addr: SocketAddr,
        server: tokio::task::JoinHandle<()>,
    }

    impl CompatibilityServer {
        async fn start() -> Self {
            std::env::set_var("CASSIE_STORAGE_MODE", "memory");
            let data_dir = data_dir("diesel_probe");
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config.clone())
                .expect("construct Cassie");
            cassie.startup().expect("start Cassie");

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(async move {
                let _ =
                    cassie::pgwire::server::run(addr.to_string(), Arc::new(cassie.clone()), config)
                        .await;
            });
            tokio::time::sleep(Duration::from_millis(50)).await;
            Self {
                data_dir,
                addr,
                server,
            }
        }

        async fn shutdown(self) {
            self.server.abort();
            let _ = self.server.await;
            let _ = std::fs::remove_dir_all(self.data_dir);
        }
    }

    fn run_external_probe(mut command: Command, timeout: Duration) -> Result<ProbeOutput, String> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| format!("spawn Diesel probe: {error}"))?;
        let started = Instant::now();
        loop {
            if child
                .try_wait()
                .map_err(|error| format!("poll Diesel probe: {error}"))?
                .is_some()
            {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("collect Diesel probe output: {error}"))?;
                return Ok(ProbeOutput {
                    success: output.status.success(),
                    timed_out: false,
                    status_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("collect timed-out Diesel probe output: {error}"))?;
                return Ok(ProbeOutput {
                    success: false,
                    timed_out: true,
                    status_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    #[ignore = "requires the pinned external Diesel fixture"]
    fn should_validate_diesel_read_model_probe_when_enabled() {
        // Arrange
        if std::env::var("CASSIE_RUN_DIESEL_COMPAT").ok().as_deref() != Some("1") {
            eprintln!("set CASSIE_RUN_DIESEL_COMPAT=1 to run the optional Diesel probe");
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let server = CompatibilityServer::start().await;
            let connection = format!(
                "postgresql://root:postgres@127.0.0.1:{}/postgres",
                server.addr.port()
            );

            // Act
            let output = tokio::task::spawn_blocking(move || {
                let mut command = Command::new("cargo");
                command.args([
                    "run",
                    "--quiet",
                    "--locked",
                    "--manifest-path",
                    "tests/fixtures/diesel_probe/Cargo.toml",
                    "--",
                    &connection,
                ]);
                run_external_probe(command, Duration::from_secs(120))
            })
            .await
            .expect("Diesel probe blocking task should complete")
            .expect("run Diesel compatibility probe");
            server.shutdown().await;

            // Assert
            assert!(
                !output.timed_out,
                "Diesel probe timed out\nstdout:\n{}\nstderr:\n{}",
                output.stdout, output.stderr
            );
            assert!(
                output.success,
                "Diesel probe failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
                output.status_code, output.stdout, output.stderr
            );
            assert!(output.stdout.contains("diesel_catalog=compat_diesel_probe"));
            assert!(output.stdout.contains("diesel_prepared=alpha"));
            assert!(output.stdout.contains("diesel_transaction_row_count=1"));
            assert!(output
                .stdout
                .contains("diesel_duplicate_error=unique_violation"));
            assert!(output
                .stdout
                .contains("diesel_missing_error=relation_missing"));
        });
    }
}

// Formerly tests/compatibility_diesel_contract.rs.
mod compatibility_diesel_contract {
    #[test]
    fn should_wire_a_pinned_diesel_fixture_into_the_opt_in_workflow() {
        // Arrange
        let workflow = include_str!("../.github/workflows/compatibility-probes.yml");
        let fixture = include_str!("fixtures/diesel_probe/Cargo.toml");
        let documentation = include_str!("../docs/compatibility-probe-contract.md");

        // Act
        let enables_probe = workflow.contains("CASSIE_RUN_DIESEL_COMPAT");
        let invokes_suite = workflow.contains("--test compatibility");
        let invokes_fixture = workflow
            .contains("compatibility_diesel::should_validate_diesel_read_model_probe_when_enabled");
        let records_client_version = workflow.contains("client_version=${DIESEL_VERSION}");
        let records_failed_probe = workflow.contains("status=failed");
        let pins_diesel = fixture.contains("diesel = { version = \"=2.2.6\"");
        let documents_probe = documentation.contains("## Diesel workflow");

        // Assert
        assert!(enables_probe);
        assert!(invokes_suite);
        assert!(invokes_fixture);
        assert!(records_client_version);
        assert!(records_failed_probe);
        assert!(pins_diesel);
        assert!(documents_probe);
    }
}

// Formerly tests/compatibility_sqlx.rs.
mod compatibility_sqlx {
    use super::support_sql as support;

    use std::net::SocketAddr;
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use support::data_dir;

    struct ProbeOutput {
        success: bool,
        timed_out: bool,
        status_code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    struct CompatibilityServer {
        data_dir: String,
        addr: SocketAddr,
        server: tokio::task::JoinHandle<()>,
    }

    impl CompatibilityServer {
        async fn start() -> Self {
            std::env::set_var("CASSIE_STORAGE_MODE", "memory");
            let data_dir = data_dir("sqlx_probe");
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.password = "postgres".to_string();
            let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config.clone())
                .expect("construct Cassie");
            cassie.startup().expect("start Cassie");

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(async move {
                let _ =
                    cassie::pgwire::server::run(addr.to_string(), Arc::new(cassie.clone()), config)
                        .await;
            });
            tokio::time::sleep(Duration::from_millis(50)).await;
            Self {
                data_dir,
                addr,
                server,
            }
        }

        async fn shutdown(self) {
            self.server.abort();
            let _ = self.server.await;
            let _ = std::fs::remove_dir_all(self.data_dir);
        }
    }

    fn run_external_probe(mut command: Command, timeout: Duration) -> Result<ProbeOutput, String> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| format!("spawn sqlx probe: {error}"))?;
        let started = Instant::now();
        loop {
            if child
                .try_wait()
                .map_err(|error| format!("poll sqlx probe: {error}"))?
                .is_some()
            {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("collect sqlx probe output: {error}"))?;
                return Ok(ProbeOutput {
                    success: output.status.success(),
                    timed_out: false,
                    status_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("collect timed-out sqlx probe output: {error}"))?;
                return Ok(ProbeOutput {
                    success: false,
                    timed_out: true,
                    status_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    #[ignore = "requires the pinned external sqlx fixture"]
    fn should_validate_sqlx_read_model_probe_when_enabled() {
        // Arrange
        if std::env::var("CASSIE_RUN_SQLX_COMPAT").ok().as_deref() != Some("1") {
            eprintln!("set CASSIE_RUN_SQLX_COMPAT=1 to run the optional sqlx probe");
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let server = CompatibilityServer::start().await;
            let connection = format!(
                "postgresql://root:postgres@127.0.0.1:{}/postgres",
                server.addr.port()
            );

            // Act
            let output = tokio::task::spawn_blocking(move || {
                let mut command = Command::new("cargo");
                command.args([
                    "run",
                    "--quiet",
                    "--locked",
                    "--manifest-path",
                    "tests/fixtures/sqlx_probe/Cargo.toml",
                    "--",
                    &connection,
                ]);
                run_external_probe(command, Duration::from_secs(120))
            })
            .await
            .expect("sqlx probe blocking task should complete")
            .expect("run sqlx compatibility probe");
            server.shutdown().await;

            // Assert
            assert!(
                !output.timed_out,
                "sqlx probe timed out\nstdout:\n{}\nstderr:\n{}",
                output.stdout, output.stderr
            );
            assert!(
                output.success,
                "sqlx probe failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
                output.status_code, output.stdout, output.stderr
            );
            assert!(output.stdout.contains("sqlx_catalog=compat_sqlx_probe"));
            assert!(output.stdout.contains("sqlx_prepared=alpha"));
            assert!(output.stdout.contains("sqlx_transaction_row_count=1"));
            assert!(output.stdout.contains("sqlx_duplicate_sqlstate=23505"));
            assert!(output.stdout.contains("sqlx_missing_sqlstate=42P01"));
        });
    }
}

// Formerly tests/compatibility_sqlx_contract.rs.
mod compatibility_sqlx_contract {
    #[test]
    fn should_wire_a_pinned_sqlx_fixture_into_the_opt_in_workflow() {
        // Arrange
        let workflow = include_str!("../.github/workflows/compatibility-probes.yml");
        let fixture = include_str!("fixtures/sqlx_probe/Cargo.toml");
        let documentation = include_str!("../docs/compatibility-probe-contract.md");

        // Act
        let enables_probe = workflow.contains("CASSIE_RUN_SQLX_COMPAT");
        let invokes_suite = workflow.contains("--test compatibility");
        let invokes_fixture = workflow
            .contains("compatibility_sqlx::should_validate_sqlx_read_model_probe_when_enabled");
        let records_client_version = workflow.contains("client_version=${SQLX_VERSION}");
        let records_failed_probe = workflow.contains("status=failed");
        let pins_sqlx = fixture.contains("sqlx = { version = \"=0.8.3\"");
        let documents_probe = documentation.contains("Dedicated opt-in fixture");

        // Assert
        assert!(enables_probe);
        assert!(invokes_suite);
        assert!(invokes_fixture);
        assert!(records_client_version);
        assert!(records_failed_probe);
        assert!(pins_sqlx);
        assert!(documents_probe);
    }
}

#[path = "support/desktop_trace.rs"]
mod support_desktop_trace;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

mod desktop_client_trace_replay {
    use super::support_desktop_trace::{
        parse_pinned_trace, parse_trace, replay_trace, validate_trace_for, DesktopClient,
    };

    const PGADMIN_TRACE: &str = include_str!("fixtures/desktop_traces/pgadmin-9.16.json");
    const DBEAVER_TRACE: &str = include_str!("fixtures/desktop_traces/dbeaver-26.1.3.json");

    #[test]
    fn should_reject_malformed_desktop_trace_json() {
        // Arrange
        let malformed = "[]";

        // Act
        let malformed_error = parse_trace(malformed).expect_err("malformed trace must fail");

        // Assert
        assert!(malformed_error.contains("valid JSON"));
    }

    #[test]
    fn should_reject_desktop_trace_without_workflow_steps() {
        // Arrange
        let incomplete = r#"{
            "schema_version": 1,
            "client": {
                "name": "pgadmin",
                "version": "9.16",
                "driver_version": null,
                "upstream_revision": "888a053231923e6704b56165cf248db056252144"
            },
            "steps": []
        }"#;

        // Act
        let incomplete_error = parse_trace(incomplete).expect_err("empty trace must fail");

        // Assert
        assert!(incomplete_error.contains("workflow steps"));
    }

    #[test]
    fn should_reject_mismatched_desktop_trace_identity_or_digest() {
        // Arrange
        let pgadmin = parse_trace(PGADMIN_TRACE).expect("valid pgAdmin fixture");
        let mismatched_version = parse_trace(&PGADMIN_TRACE.replace("9.16", "9.15"))
            .expect("structurally valid mismatched-version trace");
        let mismatched_revision = parse_trace(&PGADMIN_TRACE.replace(
            "888a053231923e6704b56165cf248db056252144",
            "0000000000000000000000000000000000000000",
        ))
        .expect("structurally valid mismatched-revision trace");

        // Act
        let mismatch_error = validate_trace_for(&pgadmin, DesktopClient::Dbeaver)
            .expect_err("client mismatch must fail");
        let version_error = validate_trace_for(&mismatched_version, DesktopClient::PgAdmin)
            .expect_err("client version mismatch must fail");
        let revision_error = validate_trace_for(&mismatched_revision, DesktopClient::PgAdmin)
            .expect_err("upstream revision mismatch must fail");
        let stale_digest_error = parse_pinned_trace(
            &PGADMIN_TRACE.replace("desktop_pgadmin_docs", "desktop_pgadmin_stale"),
            DesktopClient::PgAdmin,
        )
        .expect_err("modified trace digest must fail");

        // Assert
        assert!(mismatch_error.contains("client identity"));
        assert!(version_error.contains("client identity"));
        assert!(revision_error.contains("client identity"));
        assert!(stale_digest_error.contains("fixture digest"));
    }

    #[test]
    fn should_reject_unsupported_workflow_or_malformed_sqlstate() {
        // Arrange
        let unknown_workflow =
            PGADMIN_TRACE.replace("\"workflow\": \"plan\"", "\"workflow\": \"dashboard\"");
        let malformed_sqlstate = DBEAVER_TRACE.replace(
            "\"expected_sqlstate\": \"42P01\"",
            "\"expected_sqlstate\": \"nope?\"",
        );

        // Act
        let workflow_error =
            parse_trace(&unknown_workflow).expect_err("unknown workflow category must fail");
        let sqlstate_error =
            parse_trace(&malformed_sqlstate).expect_err("malformed SQLSTATE must fail");

        // Assert
        assert!(workflow_error.contains("unsupported workflow"));
        assert!(sqlstate_error.contains("invalid SQLSTATE"));
    }

    #[test]
    fn should_replay_pinned_pgadmin_trace() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let pgadmin = parse_pinned_trace(PGADMIN_TRACE, DesktopClient::PgAdmin)
            .expect("valid pinned pgAdmin fixture");

        // Act
        let result = runtime.block_on(replay_trace(&pgadmin));

        // Assert
        result.expect("pgAdmin trace should replay");
    }

    #[test]
    fn should_replay_pinned_dbeaver_trace() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let dbeaver = parse_pinned_trace(DBEAVER_TRACE, DesktopClient::Dbeaver)
            .expect("valid pinned DBeaver fixture");

        // Act
        let result = runtime.block_on(replay_trace(&dbeaver));

        // Assert
        result.expect("DBeaver trace should replay");
    }

    #[test]
    #[ignore = "selected by the version-pinned desktop compatibility workflow"]
    fn should_replay_selected_desktop_trace() {
        // Arrange
        let selected = std::env::var("CASSIE_DESKTOP_TRACE_CLIENT")
            .expect("CASSIE_DESKTOP_TRACE_CLIENT must select a pinned client");
        let (source, client) = match selected.as_str() {
            "pgadmin-9.16" => (PGADMIN_TRACE, DesktopClient::PgAdmin),
            "dbeaver-26.1.3" => (DBEAVER_TRACE, DesktopClient::Dbeaver),
            other => panic!("unsupported desktop trace client '{other}'"),
        };
        let trace = parse_pinned_trace(source, client).expect("valid selected pinned fixture");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        // Act
        let result = runtime.block_on(replay_trace(&trace));

        // Assert
        result.expect("selected desktop trace should replay");
    }

    #[test]
    fn should_keep_trace_replay_distinct_from_live_desktop_certification() {
        // Arrange
        let workflow = include_str!("../.github/workflows/compatibility-probes.yml");

        // Act
        let has_pinned_lanes = workflow.contains("pgadmin-9.16")
            && workflow.contains("dbeaver-26.1.3")
            && workflow.contains("POSTGRES_JDBC_VERSION: 42.7.11");
        let replays_fixture = workflow.contains("CASSIE_DESKTOP_TRACE_CLIENT")
            && workflow
                .contains("desktop_client_trace_replay::should_replay_selected_desktop_trace");
        let remains_uncertified = workflow.contains("certification_status=unavailable")
            && workflow.contains("certification_reason=no-live-desktop-evidence")
            && workflow.contains("status=replay-passed")
            && !workflow.contains("status=certified");
        let pins_cassie_revision = workflow.contains("git rev-parse HEAD")
            && workflow.contains("source revision must be the exact checked-out commit");

        // Assert
        assert!(has_pinned_lanes);
        assert!(replays_fixture);
        assert!(remains_uncertified);
        assert!(pins_cassie_revision);
    }
}
