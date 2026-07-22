use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use reqwest::StatusCode;
use tokio_postgres::NoTls;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    std::env::temp_dir()
        .join(format!(
            "cassie-role-authorization-{label}-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string()
}

#[test]
fn should_enforce_read_only_access_for_authenticated_non_admin_roles() {
    // Arrange
    with_fallback();
    let path = data_dir("statement_families");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("sa", Some("sa-secret"), None)
        .expect("admin login");
    cassie
        .execute_sql(
            &admin,
            "CREATE TABLE role_docs (title TEXT, event_at TIMESTAMP)",
            Vec::new(),
        )
        .expect("create role test table");
    cassie
        .execute_sql(
            &admin,
            "INSERT INTO role_docs (title, event_at) VALUES ('alpha', '2026-01-01T00:00:00Z')",
            Vec::new(),
        )
        .expect("seed role test table");
    cassie
        .execute_sql(
            &admin,
            "CREATE ROLE reader LOGIN PASSWORD 'reader-secret'",
            Vec::new(),
        )
        .expect("create reader role");
    let reader = cassie
        .authenticate_role("reader", Some("reader-secret"), None)
        .expect("reader login");

    // Act
    let allowed = [
        "SELECT title FROM role_docs",
        "EXPLAIN SELECT title FROM role_docs",
        "SHOW search_path",
        "SET search_path TO public",
        "BEGIN",
        "ROLLBACK",
    ]
    .into_iter()
    .map(|sql| (sql, cassie.execute_sql(&reader, sql, Vec::new())))
    .collect::<Vec<_>>();
    let forbidden = [
        "INSERT INTO role_docs (title) VALUES ('blocked')",
        "UPDATE role_docs SET title = 'blocked'",
        "DELETE FROM role_docs",
        "COPY role_docs FROM STDIN WITH (FORMAT CSV)",
        "CREATE TABLE blocked_table (title TEXT)",
        "CREATE ROLE blocked_role LOGIN PASSWORD 'blocked-secret'",
        r#"CREATE PROCEDURE blocked_proc() AS "SELECT title FROM role_docs""#,
        "CREATE MATERIALIZED PROJECTION blocked_projection AS SELECT title FROM role_docs",
        "CREATE RETENTION POLICY blocked_retention ON role_docs USING event_at RETAIN FOR '1 day'",
        "ALTER ROLE sa PASSWORD 'blocked-secret'",
    ]
    .into_iter()
    .map(|sql| (sql, cassie.execute_sql(&reader, sql, Vec::new())))
    .collect::<Vec<_>>();
    let admin_result = cassie.execute_sql(
        &admin,
        "CREATE TABLE admin_allowed (title TEXT)",
        Vec::new(),
    );
    let trusted = cassie.create_session("embedded", None);
    let trusted_result = cassie.execute_sql(
        &trusted,
        "CREATE TABLE embedded_allowed (title TEXT)",
        Vec::new(),
    );

    // Assert
    for (sql, result) in allowed {
        assert!(result.is_ok(), "reader should be allowed to execute {sql}");
    }
    for (sql, result) in forbidden {
        let error = result.expect_err("reader statement should be rejected");
        assert_eq!(
            error.to_string(),
            "insufficient privilege",
            "unexpected authorization error for {sql}"
        );
    }
    assert!(
        admin_result.is_ok(),
        "authenticated admin should retain DDL"
    );
    assert!(trusted_result.is_ok(), "embedded session should retain DDL");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_enforce_authenticated_read_only_access_through_rest() {
    // Arrange
    with_fallback();
    let path = data_dir("rest");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("sa", Some("sa-secret"), None)
        .expect("admin login");
    cassie
        .execute_sql(
            &admin,
            "CREATE TABLE rest_role_docs (title TEXT)",
            Vec::new(),
        )
        .expect("create table");
    cassie
        .execute_sql(
            &admin,
            "INSERT INTO rest_role_docs (title) VALUES ('alpha')",
            Vec::new(),
        )
        .expect("seed table");
    cassie
        .execute_sql(
            &admin,
            "CREATE ROLE rest_reader LOGIN PASSWORD 'reader-secret'",
            Vec::new(),
        )
        .expect("create reader");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rest");
        let addr = listener.local_addr().expect("rest address");
        drop(listener);
        let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie));
        tokio::time::sleep(Duration::from_millis(75)).await;
        let client = reqwest::Client::new();
        let reader_cookie = client
            .post(format!("http://{addr}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "rest_reader",
                "password": "reader-secret"
            }))
            .send()
            .await
            .expect("login request")
            .headers()
            .get("set-cookie")
            .expect("session cookie")
            .to_str()
            .expect("session cookie value")
            .split(';')
            .next()
            .expect("session cookie pair")
            .to_string();

        // Act
        let select = client
            .post(format!("http://{addr}/api/v1/admin/query/execute"))
            .header("cookie", &reader_cookie)
            .json(&serde_json::json!({"database": "postgres", "sql": "SELECT title FROM rest_role_docs"}))
            .send()
            .await
            .expect("reader select");
        let insert = client
            .post(format!("http://{addr}/api/v1/admin/query/execute"))
            .header("cookie", &reader_cookie)
            .json(&serde_json::json!({
                "database": "postgres", "sql": "INSERT INTO rest_role_docs (title) VALUES ('blocked')"
            }))
            .send()
            .await
            .expect("reader insert");
        let insert_status = insert.status();
        let insert_body = insert
            .json::<serde_json::Value>()
            .await
            .expect("insert error body");

        // Assert
        assert_eq!(select.status(), StatusCode::OK);
        assert_eq!(insert_status, StatusCode::FORBIDDEN);
        assert_eq!(insert_body["error"], "insufficient privilege");

        server.abort();
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_insufficient_privilege_sqlstate_through_pgwire() {
    // Arrange
    with_fallback();
    let path = data_dir("pgwire");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("sa", Some("sa-secret"), None)
        .expect("admin login");
    cassie
        .execute_sql(
            &admin,
            "CREATE TABLE pgwire_role_docs (title TEXT)",
            Vec::new(),
        )
        .expect("create table");
    cassie
        .execute_sql(
            &admin,
            "CREATE ROLE pgwire_reader LOGIN PASSWORD 'reader-secret'",
            Vec::new(),
        )
        .expect("create reader");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire");
        let addr = listener.local_addr().expect("pgwire address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(75)).await;
        let mut client_config = tokio_postgres::Config::new();
        client_config
            .host("127.0.0.1")
            .port(addr.port())
            .user("pgwire_reader")
            .password("reader-secret")
            .dbname("postgres");
        let (client, connection) = client_config.connect(NoTls).await.expect("connect pgwire");
        let connection = tokio::spawn(async move {
            let _ = connection.await;
        });

        // Act
        let select = client
            .simple_query("SELECT title FROM pgwire_role_docs")
            .await;
        let insert = client
            .simple_query("INSERT INTO pgwire_role_docs (title) VALUES ('blocked')")
            .await
            .expect_err("reader insert should fail");
        let copy = client
            .simple_query("COPY pgwire_role_docs FROM STDIN WITH (FORMAT CSV)")
            .await
            .expect_err("reader copy should fail before copy mode");
        let prepare = client
            .prepare("INSERT INTO pgwire_role_docs (title) VALUES ($1)")
            .await
            .expect_err("reader prepare should fail before planning");

        // Assert
        assert!(select.is_ok(), "reader select should succeed");
        assert_eq!(
            insert
                .as_db_error()
                .map(tokio_postgres::error::DbError::code),
            Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
        );
        assert_eq!(
            copy.as_db_error().map(tokio_postgres::error::DbError::code),
            Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
        );
        assert_eq!(
            prepare
                .as_db_error()
                .map(tokio_postgres::error::DbError::code),
            Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
        );

        connection.abort();
        server.abort();
        let _ = connection.await;
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
}
