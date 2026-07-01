use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use std::{fs, path::PathBuf};

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use tokio_postgres::{error::DbError, NoTls, SimpleQueryMessage};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-compatibility-matrix-{}-{}",
        label,
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

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
        with_fallback();
        let data_dir = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&data_dir).unwrap();
        cassie.startup().unwrap();

        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(async move {
            let _ = cassie::pgwire::server::run(addr.to_string(), Arc::new(cassie.clone()), config)
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
        config.user("postgres");
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
fn should_document_read_model_client_matrix() {
    // Arrange
    let docs = std::fs::read_to_string("docs/postgres-compatibility.md")
        .expect("read PostgreSQL compatibility docs");
    let required_clients = [
        "tokio-postgres",
        "psql",
        "sqlx",
        "diesel",
        "prisma",
        "SQLAlchemy",
        "migration tools",
    ];

    // Act
    let missing = required_clients
        .into_iter()
        .filter(|client| !docs.contains(client))
        .collect::<Vec<_>>();

    // Assert
    assert!(
        missing.is_empty(),
        "missing client matrix rows: {missing:?}"
    );
    assert!(docs.contains("read-model workflows"));
    assert!(docs.contains("not full PostgreSQL server equivalence"));
    assert!(docs.contains("should_validate_sqlalchemy_read_model_probe_when_enabled"));
    assert!(docs.contains("CASSIE_RUN_SQLALCHEMY_COMPAT=1"));
    assert!(docs.contains("should_validate_prisma_introspection_probe_when_enabled"));
    assert!(docs.contains("CASSIE_RUN_PRISMA_COMPAT=1"));
}

#[test]
fn should_document_pgwire_extended_query_status_without_stale_failure_claims() {
    // Arrange
    let roadmap =
        std::fs::read_to_string("docs/product-roadmap.md").expect("read product roadmap docs");
    let performance_contracts = std::fs::read_to_string("docs/performance-contracts.md")
        .expect("read performance contract docs");
    let compatibility = std::fs::read_to_string("docs/postgres-compatibility.md")
        .expect("read PostgreSQL compatibility docs");
    let docs = [
        ("docs/product-roadmap.md", roadmap.as_str()),
        (
            "docs/performance-contracts.md",
            performance_contracts.as_str(),
        ),
    ];
    let stale_claims = [
        "current pgwire behavior fails those paths",
        "spin-loop hang",
        "execute_preparsed_statement_with_mode",
    ];

    // Act
    let mut stale_matches = Vec::new();
    for (path, text) in docs {
        for claim in stale_claims {
            if text.contains(claim) {
                stale_matches.push(format!("{path}: {claim}"));
            }
        }
    }

    // Assert
    assert!(
        stale_matches.is_empty(),
        "stale pgwire status claims: {stale_matches:?}"
    );
    assert!(roadmap.contains(
        "| P0 | Repair pgwire compatibility drift between documented extended-query support and the current implementation | Implemented |"
    ));
    assert!(compatibility
        .contains("Extended query flow: parse, bind, describe, execute, sync, flush, and close."));
    assert!(compatibility.contains("sync-drain recovery"));
    assert!(performance_contracts.contains("execute_parsed_sql_with_mode"));
    assert!(performance_contracts.contains("pgwire_extended_execution.rs"));
}

#[test]
#[ignore = "requires local psql; run with CASSIE_RUN_PSQL_COMPAT=1 cargo test --locked --test compatibility_matrix should_validate_psql_read_model_probe_when_enabled -- --ignored --nocapture"]
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
        let connection = format!("postgresql://postgres@127.0.0.1:{}/postgres", server.addr.port());

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
#[ignore = "requires local Prisma CLI; run with CASSIE_RUN_PRISMA_COMPAT=1 cargo test --locked --test compatibility_matrix should_validate_prisma_introspection_probe_when_enabled -- --ignored --nocapture"]
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

        let connection = format!("postgresql://postgres@127.0.0.1:{}/postgres", server.addr.port());
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
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
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
        assert_eq!(row.get(0), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(row.get(1), Some("public"));
        assert_eq!(row.get(2), Some("postgres"));

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
        assert_eq!(version, env!("CARGO_PKG_VERSION"));

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
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
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
                "WITH RECURSIVE seq(n) AS (SELECT n FROM compat_recursive_seed WHERE n = 1 UNION ALL SELECT n FROM seq WHERE n = 1) SELECT n FROM seq ORDER BY n",
                &[],
            )
            .await
            .expect("recursive cte should succeed");

        // Assert
        let values = rows
            .into_iter()
            .map(|row| row.try_get::<_, String>(0).expect("cte value"))
            .collect::<Vec<_>>();
        assert_eq!(values, vec!["1".to_string()]);

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
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
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
        assert_eq!(version, env!("CARGO_PKG_VERSION"));

        drop(client);
        server.shutdown(connection).await;
    });
}
