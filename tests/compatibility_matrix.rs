use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use tokio_postgres::{NoTls, SimpleQueryMessage};
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

        let mut config = CassieRuntimeConfig::from_env();
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
            .execute("INSERT INTO compat_recursive_seed (n) VALUES ($1)", &[&"1"])
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
        assert!(first.is_err());
        let version: String = second.try_get(0).expect("version column");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));

        drop(client);
        server.shutdown(connection).await;
    });
}
