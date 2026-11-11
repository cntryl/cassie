use std::sync::Arc;
use std::time::Duration;

use cassie::app::{Cassie, CassieSession};
use cassie::config::CassieRuntimeConfig;
use reqwest::StatusCode;
use tokio_postgres::NoTls;

use super::support_sql as support;
use support::*;

async fn copy_out_frames_as(
    address: std::net::SocketAddr,
    user: &str,
    password: &str,
    sql: &str,
) -> Vec<(u8, Vec<u8>)> {
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect pgwire copy client");
    let (mut reader, mut writer) = socket.into_split();
    cassie_test_support::support_pgwire::complete_startup_as(
        &mut reader,
        &mut writer,
        user,
        "postgres",
        password,
    )
    .await;
    cassie_test_support::support_pgwire::write_frames(
        &mut writer,
        vec![cassie_test_support::support_pgwire::simple_query_frame(sql)],
    )
    .await;
    cassie_test_support::support_pgwire::read_frames_until_ready_within(
        &mut reader,
        Duration::from_secs(5),
    )
    .await
}

async fn copy_in_frames_as(
    address: std::net::SocketAddr,
    user: &str,
    password: &str,
    sql: &str,
    payload: &[u8],
) -> Vec<(u8, Vec<u8>)> {
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect pgwire copy client");
    let (mut reader, mut writer) = socket.into_split();
    cassie_test_support::support_pgwire::complete_startup_as(
        &mut reader,
        &mut writer,
        user,
        "postgres",
        password,
    )
    .await;
    cassie_test_support::support_pgwire::write_frames(
        &mut writer,
        vec![cassie_test_support::support_pgwire::simple_query_frame(sql)],
    )
    .await;
    let mut frames = vec![cassie_test_support::support_pgwire::read_wire_frame(&mut reader).await];
    if frames[0].0 == b'G' {
        cassie_test_support::support_pgwire::write_frames(
            &mut writer,
            vec![
                cassie_test_support::support_pgwire::copy_data_frame(payload),
                cassie_test_support::support_pgwire::copy_done_frame(),
            ],
        )
        .await;
        frames.extend(
            cassie_test_support::support_pgwire::read_frames_until_ready_within(
                &mut reader,
                Duration::from_secs(5),
            )
            .await,
        );
    }
    frames
}

fn seed_rest_role_fixture(cassie: &Cassie, admin: &CassieSession) {
    for sql in [
        "CREATE TABLE rest_role_docs (title TEXT)",
        "INSERT INTO rest_role_docs (title) VALUES ('alpha')",
        "CREATE TABLE rest_hidden_docs (title TEXT)",
        "CREATE ROLE rest_reader LOGIN PASSWORD 'reader-secret'",
        "GRANT USAGE ON SCHEMA public TO rest_reader",
        "GRANT SELECT ON TABLE public.rest_role_docs TO rest_reader",
    ] {
        cassie
            .execute_sql(admin, sql, Vec::new())
            .expect("REST role fixture SQL");
    }
}

#[test]
fn should_enforce_read_only_access_for_authenticated_non_admin_roles() {
    // Arrange
    let path = data_dir("statement_families");
    let config = CassieRuntimeConfig {
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_in_memory_with_config(config).expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some("sa-secret"), None)
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
    for grant in [
        "GRANT USAGE ON SCHEMA public TO reader",
        "GRANT SELECT ON TABLE public.role_docs TO reader",
    ] {
        cassie
            .execute_sql(&admin, grant, Vec::new())
            .expect("grant reader access");
    }
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
    let path = data_dir("rest");
    let config = CassieRuntimeConfig {
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_in_memory_with_config(config).expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some("sa-secret"), None)
        .expect("admin login");
    seed_rest_role_fixture(&cassie, &admin);

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
        let validate = client
            .post(format!("http://{addr}/api/v1/admin/query/validate"))
            .header("cookie", &reader_cookie)
            .json(&serde_json::json!({"database": "postgres", "sql": "SELECT title FROM rest_hidden_docs"}))
            .send()
            .await
            .expect("reader validation");
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
        assert_eq!(validate.status(), StatusCode::FORBIDDEN);
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
    let path = data_dir("pgwire");
    let config = CassieRuntimeConfig {
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_in_memory_with_config(config.clone()).expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some("sa-secret"), None)
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
    for grant in [
        "GRANT USAGE ON SCHEMA public TO pgwire_reader",
        "GRANT SELECT ON TABLE public.pgwire_role_docs TO pgwire_reader",
    ] {
        cassie
            .execute_sql(&admin, grant, Vec::new())
            .expect("grant reader access");
    }
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

#[test]
fn should_recheck_select_on_extended_execute_after_revoke() {
    // Arrange
    let config = support::with_test_root_password(CassieRuntimeConfig::default());
    let cassie = Arc::new(Cassie::new_in_memory_with_config(config.clone()).expect("cassie"));
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some(support::test_root_password()), None)
        .expect("admin login");
    for sql in [
        "CREATE TABLE prepared_revoke_docs (title TEXT)",
        "INSERT INTO prepared_revoke_docs (title) VALUES ('visible')",
        "CREATE ROLE prepared_reader LOGIN PASSWORD 'reader-secret'",
        "GRANT USAGE ON SCHEMA public TO prepared_reader",
        "GRANT SELECT ON TABLE public.prepared_revoke_docs TO prepared_reader",
    ] {
        cassie
            .execute_sql(&admin, sql, Vec::new())
            .expect("seed prepared-revoke fixture");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(cassie::pgwire::server::run_with_listener(
            listener,
            Arc::clone(&cassie),
            config,
        ));
        let mut client_config = tokio_postgres::Config::new();
        client_config
            .host("127.0.0.1")
            .port(address.port())
            .user("prepared_reader")
            .password("reader-secret")
            .dbname("postgres");
        let (client, connection) = client_config.connect(NoTls).await.expect("connect pgwire");
        let connection = tokio::spawn(async move {
            let _ = connection.await;
        });
        let prepared = client
            .prepare("SELECT title FROM prepared_revoke_docs")
            .await
            .expect("prepare while SELECT is granted");

        // Act
        cassie
            .execute_sql(
                &admin,
                "REVOKE SELECT ON TABLE public.prepared_revoke_docs FROM prepared_reader",
                Vec::new(),
            )
            .expect("revoke SELECT from the existing session");
        let rejected = client
            .query(&prepared, &[])
            .await
            .expect_err("extended Execute must recheck the revoked privilege");

        // Assert
        assert_eq!(
            rejected
                .as_db_error()
                .map(tokio_postgres::error::DbError::code),
            Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
        );

        connection.abort();
        server.abort();
        let _ = connection.await;
        let _ = server.await;
    });
}

#[test]
fn should_enforce_copy_privileges_by_direction() {
    // Arrange
    let config = support::with_test_root_password(CassieRuntimeConfig::default());
    let cassie = Cassie::new_in_memory_with_config(config.clone()).expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some(support::test_root_password()), None)
        .expect("admin login");
    for sql in [
        "CREATE TABLE copy_access_docs (title TEXT)",
        "INSERT INTO copy_access_docs (title) VALUES ('visible')",
        "CREATE ROLE copy_reader LOGIN PASSWORD 'reader-secret'",
        "CREATE ROLE copy_writer LOGIN PASSWORD 'writer-secret'",
        "GRANT USAGE ON SCHEMA public TO copy_reader",
        "GRANT SELECT ON TABLE public.copy_access_docs TO copy_reader",
        "GRANT USAGE ON SCHEMA public TO copy_writer",
        "GRANT INSERT ON TABLE public.copy_access_docs TO copy_writer",
    ] {
        cassie
            .execute_sql(&admin, sql, Vec::new())
            .expect("seed COPY access fixture");
    }
    let cassie = Arc::new(cassie);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(cassie::pgwire::server::run_with_listener(
            listener,
            Arc::clone(&cassie),
            config.clone(),
        ));

        // Act
        let reader_frames = copy_out_frames_as(
            address,
            "copy_reader",
            "reader-secret",
            "COPY copy_access_docs TO STDOUT WITH (FORMAT CSV)",
        )
        .await;
        let insert_only_frames = copy_out_frames_as(
            address,
            "copy_writer",
            "writer-secret",
            "COPY copy_access_docs TO STDOUT WITH (FORMAT CSV)",
        )
        .await;
        let insert_frames = copy_in_frames_as(
            address,
            "copy_writer",
            "writer-secret",
            "COPY copy_access_docs (title) FROM STDIN WITH (FORMAT CSV)",
            b"projected\n",
        )
        .await;
        let reader_payload = reader_frames
            .iter()
            .filter(|(tag, _)| *tag == b'd')
            .flat_map(|(_, payload)| payload.iter().copied())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(
            cassie_test_support::support_pgwire::error_code(&reader_frames),
            None
        );
        assert_eq!(reader_payload, b"visible\n");
        assert_eq!(
            cassie_test_support::support_pgwire::error_code(&insert_only_frames).as_deref(),
            Some("42501")
        );
        assert_eq!(insert_frames.first().map(|frame| frame.0), Some(b'G'));
        assert_eq!(
            cassie_test_support::support_pgwire::error_code(&insert_frames),
            None
        );
        assert!(insert_frames.iter().any(|(tag, payload)| {
            *tag == b'C' && payload.strip_suffix(&[0]) == Some(&b"COPY 1"[..])
        }));

        server.abort();
        let _ = server.await;
    });
}
