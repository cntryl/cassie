// Consolidated integration suite: rest.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/network_listener_authentication.rs.
mod network_listener_authentication {
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;

    use super::support_pgwire as pgwire;

    const TEST_PASSWORD: &str = "cassie-network-test-password";

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn password_config(password: &str) -> CassieRuntimeConfig {
        CassieRuntimeConfig {
            password: password.to_string(),
            ..CassieRuntimeConfig::default()
        }
    }

    async fn listener_startup_error(
        address: &str,
        cassie: Cassie,
        config: CassieRuntimeConfig,
    ) -> cassie::app::CassieError {
        tokio::time::timeout(
            Duration::from_secs(2),
            cassie::pgwire::server::run(address.to_string(), Arc::new(cassie), config),
        )
        .await
        .expect("listener validation should complete")
        .expect_err("unsafe listener must fail startup")
    }

    async fn failed_pgwire_auth_payload(address: std::net::SocketAddr) -> Vec<u8> {
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        tokio::io::AsyncWriteExt::write_all(
            &mut writer,
            &pgwire::startup_frame("root", "postgres"),
        )
        .await
        .expect("write startup");
        tokio::io::AsyncWriteExt::flush(&mut writer)
            .await
            .expect("flush startup");
        let authentication = pgwire::read_wire_frame(&mut reader).await;
        assert_eq!(authentication.0, b'R');
        tokio::io::AsyncWriteExt::write_all(&mut writer, &pgwire::password_message("wrong"))
            .await
            .expect("write password");
        tokio::io::AsyncWriteExt::flush(&mut writer)
            .await
            .expect("flush password");
        let error = pgwire::read_wire_frame(&mut reader).await;
        assert_eq!(error.0, b'E');
        error.1
    }

    #[test]
    fn should_allow_passwordless_bootstrap_for_embedded_use_without_listeners() {
        // Arrange
        pgwire::use_local_storage();
        let path = pgwire::data_dir("embedded-passwordless");
        let config = password_config("");
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");

        // Act
        let session = cassie.authenticate_role("root", None, None);

        // Assert
        assert!(session.is_ok());
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_passwordless_pgwire_listener_at_actual_loopback_address() {
        // Arrange
        pgwire::use_local_storage();
        let path = pgwire::data_dir("listener-passwordless");
        let config = password_config("");
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
        cassie.startup().expect("startup");

        runtime().block_on(async {
            // Act
            let error = listener_startup_error("localhost:0", cassie, config).await;

            // Assert
            assert!(error.to_string().contains("bootstrap password is empty"));
            assert!(error.to_string().contains("network listener"));
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_passwordless_rest_listener_at_actual_loopback_address() {
        // Arrange
        pgwire::use_local_storage();
        let path = pgwire::data_dir("rest-listener-passwordless");
        let config = password_config("");
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");

        runtime().block_on(async {
            // Act
            let error = tokio::time::timeout(
                Duration::from_secs(2),
                cassie::rest::router::run("127.0.0.1:0".to_string(), cassie),
            )
            .await
            .expect("listener validation should complete")
            .expect_err("passwordless REST listener must fail startup");

            // Assert
            assert!(error.to_string().contains("bootstrap password is empty"));
            assert!(error.to_string().contains("network listener"));
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rotate_persisted_passwordless_role_before_pgwire_listener_startup() {
        // Arrange
        pgwire::use_local_storage();
        let path = pgwire::data_dir("persisted-passwordless");
        {
            let cassie = Cassie::new_with_data_dir_and_config(&path, password_config(""))
                .expect("passwordless cassie");
            cassie.startup().expect("passwordless startup");
            cassie.shutdown();
        }
        let config = password_config(TEST_PASSWORD);
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
        cassie.startup().expect("startup");

        runtime().block_on(async {
            // Act
            let authenticated = cassie.authenticate_role("root", Some(TEST_PASSWORD), None);

            // Assert
            assert!(authenticated.is_ok());
            let shutdown = Arc::new(tokio::sync::Notify::new());
            let shutdown_signal = shutdown.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                shutdown_signal.notify_waiters();
            });
            cassie::pgwire::server::run_with_shutdown(
                "127.0.0.1:0".to_string(),
                Arc::new(cassie),
                config,
                shutdown,
            )
            .await
            .expect("rotated credentials permit pgwire listener");
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rotate_persisted_passwordless_role_before_rest_listener_startup() {
        // Arrange
        pgwire::use_local_storage();
        let path = pgwire::data_dir("rest-persisted-passwordless");
        {
            let cassie = Cassie::new_with_data_dir_and_config(&path, password_config(""))
                .expect("passwordless cassie");
            cassie.startup().expect("passwordless startup");
            cassie.shutdown();
        }
        let config = password_config(TEST_PASSWORD);
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");

        runtime().block_on(async {
            // Act
            let authenticated = cassie.authenticate_role("root", Some(TEST_PASSWORD), None);

            // Assert
            assert!(authenticated.is_ok());
            let shutdown = Arc::new(tokio::sync::Notify::new());
            let shutdown_signal = shutdown.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                shutdown_signal.notify_waiters();
            });
            cassie::rest::router::run_with_shutdown("127.0.0.1:0".to_string(), cassie, shutdown)
                .await
                .expect("rotated credentials permit REST listener");
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_default_postgres_password_loopback_only_for_runtime_address() {
        // Arrange
        pgwire::use_local_storage();
        let path = pgwire::data_dir("default-password-non-loopback");
        let config = CassieRuntimeConfig::default();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
        cassie.startup().expect("startup");

        runtime().block_on(async {
            // Act
            let error = listener_startup_error("0.0.0.0:0", cassie, config).await;

            // Assert
            assert!(error
                .to_string()
                .contains("default bootstrap password is unsafe"));
            assert!(error.to_string().contains("0.0.0.0:0"));
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_require_tls_for_actual_non_loopback_pgwire_listener() {
        // Arrange
        pgwire::use_local_storage();
        let path = pgwire::data_dir("non-loopback-tls");
        let config = password_config(TEST_PASSWORD);
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
        cassie.startup().expect("startup");

        runtime().block_on(async {
            // Act
            let error = listener_startup_error("0.0.0.0:0", cassie, config).await;

            // Assert
            assert!(error.to_string().contains("pgwire TLS is required"));
            assert!(error.to_string().contains("0.0.0.0:0"));
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_authenticate_non_empty_password_on_loopback_pgwire_listener() {
        // Arrange
        pgwire::use_local_storage();
        let path = pgwire::data_dir("loopback-authenticated");
        let config = password_config(TEST_PASSWORD);
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
        cassie.startup().expect("startup");

        runtime().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("reserve listener");
            let address = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(cassie::pgwire::server::run_with_shutdown(
                address.to_string(),
                Arc::new(cassie),
                config,
                Arc::new(tokio::sync::Notify::new()),
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;
            let socket = tokio::net::TcpStream::connect(address)
                .await
                .expect("connect pgwire");
            let (mut reader, mut writer) = tokio::io::split(socket);

            // Act
            pgwire::complete_startup_with_password(&mut reader, &mut writer, TEST_PASSWORD).await;

            // Assert
            assert!(!server.is_finished());
            server.abort();
            let _ = server.await;
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_pgwire_authentication_envelope_generic_when_throttled() {
        // Arrange
        pgwire::use_local_storage();
        let path = pgwire::data_dir("pgwire-auth-throttle");
        let config = CassieRuntimeConfig {
            password: TEST_PASSWORD.to_string(),
            auth_user_attempts_per_minute: 1,
            auth_ip_attempts_per_minute: 10,
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
        cassie.startup().expect("startup");

        runtime().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("reserve listener");
            let address = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(cassie::pgwire::server::run_with_shutdown(
                address.to_string(),
                Arc::new(cassie),
                config,
                Arc::new(tokio::sync::Notify::new()),
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Act
            let invalid = failed_pgwire_auth_payload(address).await;
            let throttled = failed_pgwire_auth_payload(address).await;

            // Assert
            assert_eq!(throttled, invalid);
            assert!(String::from_utf8_lossy(&throttled).contains("authentication failed"));
            server.abort();
            let _ = server.await;
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/rest.rs.
mod rest {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::rest::{collections, documents};
    use uuid::Uuid;

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_crud_collection_documents_through_rest() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("crud");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "rest_docs";
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let body = serde_json::json!({
                "name": collection,
                "fields": [
                    {"name": "title", "type": "text"},
                    {"name": "payload", "type": "json"},
                    {"name": "embedding", "type": "vector(2)"},
                ]
            });

            // Act
            let create = collections::create(&cassie, body.to_string().as_bytes())
                .expect("create collection");
            let list = collections::list(&cassie);
            let doc = documents::create(
                &cassie,
                collection,
                serde_json::json!({"title": "hello", "payload": {"k": 1}, "embedding": [1.0, 2.0]})
                    .to_string()
                    .as_bytes(),
            )
            .expect("create document");
            let doc_id = doc["id"].as_str().expect("id present");
            let got = documents::get(&cassie, collection, doc_id).expect("get document");
            let removed = documents::delete(&cassie, collection, doc_id).expect("delete document");

            // Assert
            assert_eq!(create["collection"], collection);
            assert!(list.contains(&canonical_relation_name("postgres", "public", collection)));
            assert_eq!(got["title"], "hello");
            assert_eq!(removed["deleted"], serde_json::Value::Bool(true));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_invalid_vector_dimensions_through_rest() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("bad_vector");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "rest_bad_vector";
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let _ = collections::create(
                &cassie,
                serde_json::json!({
                    "name": collection,
                    "fields": [{"name": "embedding", "type": "vector(2)"}],
                })
                .to_string()
                .as_bytes(),
            );

            // Act
            let insert = documents::create(
                &cassie,
                collection,
                serde_json::json!({"embedding": [1.0, 2.0, 3.0]})
                    .to_string()
                    .as_bytes(),
            );

            // Assert
            assert!(insert.is_err(), "dimension mismatch should fail");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_missing_document_lookup_through_rest() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("missing_doc");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "rest_missing_doc";
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let _ = collections::create(
                &cassie,
                serde_json::json!({
                    "name": collection,
                    "fields": [{"name": "title", "type": "text"}],
                })
                .to_string()
                .as_bytes(),
            );

            // Act
            let missing = documents::get(&cassie, collection, "missing-id");
            // Assert
            assert!(missing.is_err(), "missing document should fail");
            let error = missing.unwrap_err().to_string();
            assert!(
                error.contains("document not found"),
                "unexpected error: {error}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_apply_default_values_for_rest_ingest() {
        // Arrange
        let path = format!("/tmp/cassie-rest-default-{}", Uuid::new_v4());
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "rest_constraint_defaults";
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_defaults (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )

.unwrap();

        // Act
        let doc = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1}).to_string().as_bytes(),
        )
        .expect("create rest document");
        let id = doc["id"].as_str().expect("id present");
        let stored = cassie
            .midge
            .get_document(collection, id)

            .expect("document read");

        // Assert
        let stored = stored.expect("document should be stored").payload;
        assert_eq!(stored["status"], "pending");
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_rest_ingest_when_not_null_constraint_is_violated() {
        // Arrange
        let path = format!("/tmp/cassie-rest-not-null-{}", Uuid::new_v4());
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "rest_constraint_not_null";
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let session = cassie.create_session("root", None);
            cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_not_null (id INT PRIMARY KEY, email TEXT NOT NULL)",
                vec![],
            )
            .unwrap();

            // Act
            let missing = documents::create(
                &cassie,
                collection,
                serde_json::json!({"id": 1}).to_string().as_bytes(),
            );

            // Assert
            assert!(
                missing.is_err(),
                "missing required field should be rejected"
            );
            let error = missing.unwrap_err().to_string();
            assert!(
                error.contains("cannot be null"),
                "unexpected error: {error}"
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_rest_ingest_when_unique_constraint_is_violated() {
        // Arrange
        let path = format!("/tmp/cassie-rest-unique-{}", Uuid::new_v4());
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "rest_constraint_unique";
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_unique (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE)",
                vec![],
            )

.unwrap();

        documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1, "email": "a@example.com"})
                .to_string()
                .as_bytes(),
        )
        .expect("first insert");

        // Act
        let duplicate = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 2, "email": "a@example.com"})
                .to_string()
                .as_bytes(),
        );

        // Assert
        assert!(duplicate.is_err(), "duplicate unique field should be rejected");
        let error = duplicate.unwrap_err().to_string();
        assert!(
            error.contains("unique constraint failed"),
            "unexpected error: {error}"
        );
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_rest_ingest_when_check_constraint_is_violated() {
        // Arrange
        let path = format!("/tmp/cassie-rest-check-{}", Uuid::new_v4());
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "rest_constraint_check";
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_check (id INT PRIMARY KEY, score INT CHECK (score >= 18))",
                vec![],
            )

.unwrap();

        // Act
        let invalid = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1, "score": 17})
                .to_string()
                .as_bytes(),
        );

        // Assert
        assert!(invalid.is_err(), "check constraint failure should be rejected");
        let error = invalid.unwrap_err().to_string();
        assert!(
            error.contains("check constraint failed"),
            "unexpected error: {error}"
        );
    });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/rest_admin_databases.rs.
mod rest_admin_databases {
    use std::path::PathBuf;
    use std::sync::Arc;

    use cassie::app::{Cassie, CassieError};
    use cassie::catalog::canonical_schema_name;
    use reqwest::{Client, StatusCode};
    use tokio::sync::Notify;
    use uuid::Uuid;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn data_dir(label: &str) -> PathBuf {
        crate::support_temp_dirs::sweep_stale_once();
        std::env::temp_dir().join(format!(
            "cassie-rest-admin-databases-{label}-{}",
            Uuid::new_v4()
        ))
    }

    async fn spawn_rest_server(
        cassie: Cassie,
    ) -> (
        String,
        Arc<Notify>,
        tokio::task::JoinHandle<Result<(), CassieError>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);

        let shutdown = Arc::new(Notify::new());
        let server = tokio::spawn(cassie::rest::router::run_with_shutdown(
            address.to_string(),
            cassie,
            shutdown.clone(),
        ));
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;

        (format!("http://{address}"), shutdown, server)
    }

    async fn stop_rest_server(
        shutdown: Arc<Notify>,
        server: tokio::task::JoinHandle<Result<(), CassieError>>,
    ) {
        shutdown.notify_waiters();
        let _ = server.await;
    }

    async fn login_cookie(
        client: &Client,
        base_url: &str,
        username: &str,
        password: &str,
    ) -> String {
        client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": username,
                "password": password,
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
            .to_string()
    }

    #[test]
    fn should_create_admin_database_through_dedicated_rest_endpoint() {
        // Arrange
        use_local_storage();
        let path = data_dir("create");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let observable = cassie.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let cookie = login_cookie(&client, &base_url, "root", "postgres").await;

            // Act
            let response = client
                .post(format!("{base_url}/api/v1/admin/databases"))
                .header("cookie", cookie)
                .json(&serde_json::json!({ "name": " Analytics_1 " }))
                .send()
                .await
                .expect("create database");

            // Assert
            assert_eq!(response.status(), StatusCode::CREATED);
            assert_eq!(
                response
                    .json::<serde_json::Value>()
                    .await
                    .expect("database summary"),
                serde_json::json!({ "name": "analytics_1" })
            );
            assert!(observable.catalog.database_exists("analytics_1"));
            assert!(observable
                .catalog
                .namespace_exists(&canonical_schema_name("analytics_1", "public")));
            assert!(observable
                .midge
                .get_database("analytics_1")
                .expect("database metadata")
                .is_some());
            assert!(observable
                .midge
                .list_namespaces()
                .iter()
                .any(|name| name == "analytics_1.public"));

            stop_rest_server(shutdown, server).await;
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_duplicate_admin_database_names() {
        // Arrange
        use_local_storage();
        let path = data_dir("duplicate");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let cookie = login_cookie(&client, &base_url, "root", "postgres").await;
            let first = client
                .post(format!("{base_url}/api/v1/admin/databases"))
                .header("cookie", &cookie)
                .json(&serde_json::json!({ "name": "analytics" }))
                .send()
                .await
                .expect("first create database");
            assert_eq!(first.status(), StatusCode::CREATED);

            // Act
            let duplicate = client
                .post(format!("{base_url}/api/v1/admin/databases"))
                .header("cookie", cookie)
                .json(&serde_json::json!({ "name": "analytics" }))
                .send()
                .await
                .expect("duplicate create database");

            // Assert
            assert_eq!(duplicate.status(), StatusCode::CONFLICT);
            assert_eq!(
                duplicate
                    .json::<serde_json::Value>()
                    .await
                    .expect("duplicate error")["error"],
                "database 'analytics' already exists"
            );

            stop_rest_server(shutdown, server).await;
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_malformed_admin_database_names() {
        // Arrange
        use_local_storage();
        let path = data_dir("invalid");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let cookie = login_cookie(&client, &base_url, "root", "postgres").await;

            // Act
            let mut statuses = Vec::new();
            for name in ["", "tenant.analytics", "9analytics", "analytics-reporting"] {
                statuses.push(
                    client
                        .post(format!("{base_url}/api/v1/admin/databases"))
                        .header("cookie", &cookie)
                        .json(&serde_json::json!({ "name": name }))
                        .send()
                        .await
                        .expect("invalid create database")
                        .status(),
                );
            }

            // Assert
            assert_eq!(statuses, vec![StatusCode::BAD_REQUEST; 4]);

            stop_rest_server(shutdown, server).await;
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_require_admin_authorization_to_create_database() {
        // Arrange
        use_local_storage();
        let path = data_dir("authorization");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let root = cassie
            .authenticate_role("root", Some("postgres"), None)
            .expect("root session");
        cassie
            .execute_sql(
                &root,
                "CREATE ROLE reader LOGIN PASSWORD 'reader-secret'",
                Vec::new(),
            )
            .expect("create reader");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let reader_cookie = login_cookie(&client, &base_url, "reader", "reader-secret").await;

            // Act
            let unauthorized = client
                .post(format!("{base_url}/api/v1/admin/databases"))
                .json(&serde_json::json!({ "name": "unauthorized_database" }))
                .send()
                .await
                .expect("unauthorized create database");
            let forbidden = client
                .post(format!("{base_url}/api/v1/admin/databases"))
                .header("cookie", reader_cookie)
                .json(&serde_json::json!({ "name": "forbidden_database" }))
                .send()
                .await
                .expect("forbidden create database");

            // Assert
            assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

            stop_rest_server(shutdown, server).await;
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/rest_admin_query.rs.
mod rest_admin_query {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::{Cassie, CassieError};
    use cassie::catalog::canonical_relation_name;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::{DataType, FieldSchema, Schema};
    use reqwest::{Client, StatusCode};
    use tokio::sync::Notify;
    use uuid::Uuid;

    type QueryEndpointCase = (reqwest::Method, String, Option<serde_json::Value>);

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn data_dir(label: &str) -> PathBuf {
        crate::support_temp_dirs::sweep_stale_once();
        std::env::temp_dir().join(format!(
            "cassie-rest-admin-query-{label}-{}",
            Uuid::new_v4()
        ))
    }

    async fn spawn_rest_server(
        cassie: Cassie,
    ) -> (
        String,
        Arc<Notify>,
        tokio::task::JoinHandle<Result<(), CassieError>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let shutdown = Arc::new(Notify::new());
        let server = tokio::spawn(cassie::rest::router::run_with_shutdown(
            addr.to_string(),
            cassie,
            shutdown.clone(),
        ));
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;

        (format!("http://{addr}"), shutdown, server)
    }

    async fn stop_rest_server(
        shutdown: Arc<Notify>,
        server: tokio::task::JoinHandle<Result<(), CassieError>>,
    ) {
        shutdown.notify_waiters();
        let _ = server.await;
    }

    fn seed_query_catalog(cassie: &Cassie) {
        let session = cassie
            .authenticate_role("root", Some("postgres"), None)
            .expect("admin session");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_admin_query_docs (id INT PRIMARY KEY, title TEXT)",
                Vec::new(),
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO rest_admin_query_docs (id, title) VALUES (1, 'alpha')",
                Vec::new(),
            )
            .expect("insert document");
        cassie
        .execute_sql(
            &session,
            "CREATE INDEX rest_admin_query_title_idx ON rest_admin_query_docs USING btree (title)",
            Vec::new(),
        )
        .expect("create index");
        cassie
            .execute_sql(
                &session,
                "CREATE VIEW rest_admin_query_ready AS SELECT title FROM rest_admin_query_docs",
                Vec::new(),
            )
            .expect("create view");
        cassie
            .execute_sql(
                &session,
                "CREATE FUNCTION rest_query_identity(x INT) RETURNS INT AS \"x\"",
                Vec::new(),
            )
            .expect("create function");
        cassie
        .execute_sql(
            &session,
            r#"CREATE PROCEDURE rest_query_store(title TEXT) AS "INSERT INTO rest_admin_query_docs (id, title) VALUES (2, $1)""#,
            Vec::new(),
        )
        .expect("create procedure");
    }

    fn query_endpoint_cases(base_url: &str) -> Vec<QueryEndpointCase> {
        vec![
            (
                reqwest::Method::GET,
                format!("{base_url}/api/v1/admin/query/schema?database=postgres"),
                None,
            ),
            (
                reqwest::Method::POST,
                format!("{base_url}/api/v1/admin/query/execute"),
                Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
            ),
            (
                reqwest::Method::POST,
                format!("{base_url}/api/v1/admin/query/validate"),
                Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
            ),
            (
                reqwest::Method::POST,
                format!("{base_url}/api/v1/admin/query/explain"),
                Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
            ),
            (
                reqwest::Method::GET,
                format!("{base_url}/api/v1/admin/catalog?database=postgres"),
                None,
            ),
            (
                reqwest::Method::POST,
                format!("{base_url}/api/v1/admin/query-executions"),
                Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
            ),
            (
                reqwest::Method::POST,
                format!("{base_url}/api/v1/admin/query-validations"),
                Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
            ),
            (
                reqwest::Method::POST,
                format!("{base_url}/api/v1/admin/query-explanations"),
                Some(serde_json::json!({"database": "postgres", "sql": "SELECT 1"})),
            ),
        ]
    }

    fn section_items<'a>(
        schema: &'a serde_json::Value,
        section_id: &str,
    ) -> &'a [serde_json::Value] {
        schema["sections"]
            .as_array()
            .expect("schema sections")
            .iter()
            .find(|section| section["id"] == section_id)
            .and_then(|section| section["items"].as_array())
            .map(Vec::as_slice)
            .expect("section items")
    }

    fn contains_item(items: &[serde_json::Value], label: &str) -> bool {
        items.iter().any(|item| item["label"] == label)
    }

    fn plan_feature_enabled(plan: &serde_json::Value, feature_id: &str) -> bool {
        plan["features"]
            .as_array()
            .expect("plan features")
            .iter()
            .any(|feature| feature["id"] == feature_id && feature["enabled"] == true)
    }

    async fn login_cookie(client: &Client, base_url: &str) -> String {
        client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "root",
                "password": "postgres"
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
            .to_string()
    }

    async fn post_admin_query(
        client: &Client,
        base_url: &str,
        path: &str,
        sql: &str,
    ) -> reqwest::Response {
        let session_cookie = login_cookie(client, base_url).await;
        client
            .post(format!("{base_url}{path}"))
            .header("cookie", session_cookie)
            .json(&serde_json::json!({ "database": "postgres", "sql": sql }))
            .send()
            .await
            .expect("admin query request")
    }

    #[test]
    fn should_scope_each_admin_request_to_its_explicit_database() {
        // Arrange
        use_local_storage();
        let cassie =
            Cassie::new_with_data_dir(data_dir("explicit-database-scope")).expect("cassie");
        let admin = cassie
            .authenticate_role("root", Some("postgres"), None)
            .expect("admin session");
        cassie
            .execute_sql(&admin, "CREATE DATABASE analytics", Vec::new())
            .expect("create analytics database");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let cookie = login_cookie(&client, &base_url).await;

            // Act
            let databases = client
                .get(format!("{base_url}/api/v1/admin/databases"))
                .header("cookie", &cookie)
                .send()
                .await
                .expect("database discovery");
            let postgres = client
            .post(format!("{base_url}/api/v1/admin/query-executions"))
            .header("cookie", &cookie)
            .json(&serde_json::json!({"database": "postgres", "sql": "SELECT current_database()"}))
            .send()
            .await
            .expect("postgres query");
            let analytics = client
            .post(format!("{base_url}/api/v1/admin/query-executions"))
            .header("cookie", &cookie)
            .json(&serde_json::json!({"database": "analytics", "sql": "SELECT current_database()"}))
            .send()
            .await
            .expect("analytics query");
            let missing = client
                .post(format!("{base_url}/api/v1/admin/query-validations"))
                .header("cookie", &cookie)
                .json(&serde_json::json!({"sql": "SELECT 1"}))
                .send()
                .await
                .expect("missing database");
            let unknown = client
                .get(format!("{base_url}/api/v1/admin/catalog?database=missing"))
                .header("cookie", &cookie)
                .send()
                .await
                .expect("unknown database");

            // Assert
            assert_eq!(databases.status(), StatusCode::OK);
            let discovered = databases
                .json::<serde_json::Value>()
                .await
                .expect("database json");
            assert_eq!(discovered[0]["name"], "analytics");
            assert_eq!(discovered[1]["name"], "postgres");
            assert_eq!(
                postgres
                    .json::<serde_json::Value>()
                    .await
                    .expect("postgres json")["rows"][0][0],
                "postgres"
            );
            assert_eq!(
                analytics
                    .json::<serde_json::Value>()
                    .await
                    .expect("analytics json")["rows"][0][0],
                "analytics"
            );
            assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
            assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
            stop_rest_server(shutdown, server).await;
        });
    }

    #[test]
    fn should_reject_unauthorized_access_to_admin_query_routes() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("unauthorized");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            cassie.startup().expect("startup");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let cases = query_endpoint_cases(base_url.as_str());

            for (method, url, body) in cases {
                let request = client.request(method, url);
                let request = if let Some(body) = body {
                    request.json(&body)
                } else {
                    request
                };

                // Act
                let response = request.send().await.expect("query request");

                // Assert
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            }

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_execute_admin_query_through_rest() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("execute");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        cassie.startup().expect("startup");
        seed_query_catalog(&cassie);
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let admin_cookie = login_cookie(&client, &base_url).await;

        // Act
        let response = client
            .post(format!("{base_url}/api/v1/admin/query/execute"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "database": "postgres", "sql": "SELECT title FROM rest_admin_query_docs ORDER BY title"
            }))
            .send()
            .await
            .expect("execute request");
        let status = response.status();
        let payload = response.json::<serde_json::Value>().await.expect("json");

        // Assert
        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["command"], "SELECT");
        assert_eq!(payload["columns"][0]["name"], "title");
        assert_eq!(payload["rows"].as_array().expect("rows").len(), 1);

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
    }

    #[test]
    fn should_complete_admin_query_workflow_given_one_authenticated_session() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("authenticated-workflow");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            cassie.startup().expect("startup");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let admin_cookie = login_cookie(&client, &base_url).await;

            // Act
            let mut responses = Vec::new();
            for sql in [
                "CREATE TABLE ui_demo (demo_id INT PRIMARY KEY, name TEXT NOT NULL)",
                "INSERT INTO ui_demo (demo_id, name) VALUES (1, 'Ada'), (2, 'Grace')",
                "SELECT demo_id, name FROM ui_demo ORDER BY demo_id",
            ] {
                let response = client
                    .post(format!("{base_url}/api/v1/admin/query-executions"))
                    .header("cookie", &admin_cookie)
                    .json(&serde_json::json!({ "database": "postgres", "sql": sql }))
                    .send()
                    .await
                    .expect("query execution");
                responses.push((
                    response.status(),
                    response.json::<serde_json::Value>().await.expect("json"),
                ));
            }

            // Assert
            assert_eq!(responses[0].0, StatusCode::OK);
            assert_eq!(responses[0].1["command"], "CREATE TABLE");
            assert_eq!(responses[1].0, StatusCode::OK);
            assert_eq!(responses[1].1["command"], "INSERT 0 2");
            assert_eq!(responses[2].0, StatusCode::OK);
            assert_eq!(responses[2].1["command"], "SELECT");
            assert_eq!(
                responses[2].1["rows"],
                serde_json::json!([[1, "Ada"], [2, "Grace"]])
            );

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_validate_admin_query_through_rest() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("validate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            cassie.startup().expect("startup");
            seed_query_catalog(&cassie);
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let admin_cookie = login_cookie(&client, &base_url).await;

            // Act
            let valid = client
                .post(format!("{base_url}/api/v1/admin/query/validate"))
                .header("cookie", &admin_cookie)
                .json(&serde_json::json!({
                    "database": "postgres", "sql": "SELECT title FROM rest_admin_query_docs"
                }))
                .send()
                .await
                .expect("valid request");
            let valid_status = valid.status();
            let valid_payload = valid.json::<serde_json::Value>().await.expect("valid json");
            let malformed = client
                .post(format!("{base_url}/api/v1/admin/query/validate"))
                .header("cookie", &admin_cookie)
                .json(&serde_json::json!({"database": "postgres", "sql": "SELECT FROM"}))
                .send()
                .await
                .expect("malformed request");
            let malformed_status = malformed.status();
            let malformed_payload = malformed
                .json::<serde_json::Value>()
                .await
                .expect("malformed json");

            // Assert
            assert_eq!(valid_status, StatusCode::OK);
            assert_eq!(valid_payload["valid"].as_bool(), Some(true));
            assert_eq!(valid_payload["command"], "SELECT");
            assert_eq!(valid_payload["columns"][0]["name"], "title");
            assert_eq!(malformed_status, StatusCode::BAD_REQUEST);
            assert!(malformed_payload["error"].as_str().is_some());

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_map_admin_query_errors_to_semantic_http_statuses() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("query-errors");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            cassie.startup().expect("startup");
            seed_query_catalog(&cassie);
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();

            // Act
            let malformed = post_admin_query(
                &client,
                &base_url,
                "/api/v1/admin/query/execute",
                "SELECT FROM",
            )
            .await;
            let missing = post_admin_query(
                &client,
                &base_url,
                "/api/v1/admin/query/execute",
                "SELECT title FROM missing_rest_admin_query_docs",
            )
            .await;
            let unsupported = post_admin_query(
                &client,
                &base_url,
                "/api/v1/admin/query/execute",
                "COPY rest_admin_query_docs TO STDOUT",
            )
            .await;
            let malformed_status = malformed.status();
            let missing_status = missing.status();
            let unsupported_status = unsupported.status();
            let malformed_payload = malformed
                .json::<serde_json::Value>()
                .await
                .expect("malformed");
            let missing_payload = missing.json::<serde_json::Value>().await.expect("missing");
            let unsupported_payload = unsupported
                .json::<serde_json::Value>()
                .await
                .expect("unsupported");

            // Assert
            assert_eq!(malformed_status, StatusCode::BAD_REQUEST);
            assert_eq!(missing_status, StatusCode::NOT_FOUND);
            assert_eq!(unsupported_status, StatusCode::NOT_IMPLEMENTED);
            assert!(malformed_payload["error"]
                .as_str()
                .expect("malformed error")
                .contains("SELECT"));
            assert!(missing_payload["error"]
                .as_str()
                .expect("missing error")
                .contains("missing_rest_admin_query_docs"));
            assert!(unsupported_payload["error"]
                .as_str()
                .expect("unsupported error")
                .contains("unsupported feature"));

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_return_gateway_timeout_for_admin_query_deadlines() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("deadline");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.query_timeout_ms = 1;
            let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config)
                .expect("cassie with config");
            cassie.startup().expect("startup");
            let collection =
                canonical_relation_name("postgres", "public", "rest_admin_timeout_docs");
            cassie
                .midge
                .create_collection(
                    &collection,
                    Schema {
                        fields: vec![FieldSchema {
                            name: "title".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        }],
                    },
                )
                .expect("create timeout collection");
            cassie.register_collection(
                &collection,
                Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            );
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .expect("seed timeout document");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let sql = format!(
                "{}SELECT title FROM rest_admin_timeout_docs",
                " ".repeat(1_000_000)
            );

            // Act
            let response =
                post_admin_query(&client, &base_url, "/api/v1/admin/query/execute", &sql).await;
            let status = response.status();
            let payload = response
                .json::<serde_json::Value>()
                .await
                .expect("timeout payload");

            // Assert
            assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
            assert_eq!(payload["error"], "query timeout exceeded");

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_acknowledge_admin_query_cancellation_before_returning() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("operation-cancellation");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.cte_recursion_depth = 1_000_000;
        config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config).expect("cassie");
        cassie.startup().expect("startup");
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let admin_cookie = login_cookie(&client, &base_url).await;
        let operation_id = uuid::Uuid::new_v4();
        let query_client = client.clone();
        let query_url = format!("{base_url}/api/v1/admin/query-executions");
        let query_cookie = admin_cookie.clone();

        // Act
        let query = tokio::spawn(async move {
            query_client
                .post(query_url)
                .header("cookie", query_cookie)
                .json(&serde_json::json!({
                    "operation_id": operation_id,
                    "database": "postgres", "sql": "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 1000000) SELECT MAX(n) FROM seq"
                }))
                .send()
                .await
                .expect("query response")
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        let cancellation = client
            .delete(format!(
                "{base_url}/api/v1/admin/query-operations/{operation_id}"
            ))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .expect("cancellation response");
        let cancellation_status = cancellation.status();
        let cancellation_payload = cancellation
            .json::<serde_json::Value>()
            .await
            .expect("cancellation payload");
        let query_status = query.await.expect("query task").status();

        // Assert
        assert_eq!(cancellation_status, StatusCode::OK);
        assert_eq!(cancellation_payload["operation_id"], operation_id.to_string());
        assert_eq!(cancellation_payload["cancelled"], true);
        assert_eq!(query_status.as_u16(), 499);

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
    }

    #[test]
    fn should_explain_admin_query_through_rest() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("explain");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        cassie.startup().expect("startup");
        seed_query_catalog(&cassie);
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let admin_cookie = login_cookie(&client, &base_url).await;

        // Act
        let response = client
            .post(format!("{base_url}/api/v1/admin/query/explain"))
            .header("cookie", &admin_cookie)
            .json(&serde_json::json!({
                "database": "postgres", "sql": "SELECT title FROM rest_admin_query_docs WHERE title = 'alpha'"
            }))
            .send()
            .await
            .expect("explain request");
        let status = response.status();
        let payload = response.json::<serde_json::Value>().await.expect("json");

        // Assert
        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["command"], "EXPLAIN");
        assert_eq!(payload["columns"][0]["name"], "QUERY PLAN");
        assert_eq!(payload["plan"]["format_version"], 1);
        assert!(payload["plan"]["summary"]["collection"]
            .as_str()
            .expect("plan collection")
            .ends_with("rest_admin_query_docs"));
        assert_eq!(payload["plan"]["summary"]["access_path"], "index_seek");
        assert_eq!(
            payload["plan"]["summary"]["selected_index"],
            canonical_relation_name("postgres", "public", "rest_admin_query_title_idx")
        );
        assert_eq!(payload["plan"]["nodes"][0]["kind"], "read");
        assert_eq!(payload["plan"]["nodes"][0]["status"], "optimized");
        assert!(plan_feature_enabled(&payload["plan"], "predicate_pushdown"));
        assert!(plan_feature_enabled(&payload["plan"], "covered_index"));
        assert_eq!(
            payload["plan"]["diagnostics"]["access_path_reason"],
            "scalar-index-seek"
        );
        assert!(
            payload["plan"]["estimates"]["selected_cost"]
                .as_u64()
                .expect("selected cost")
                > 0
        );
        assert!(
            !payload["rows"].as_array().expect("rows").is_empty(),
            "explain should return at least one plan row"
        );

        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
    });
    }

    #[test]
    fn should_return_admin_query_schema_sections_in_stable_order() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("schema");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            cassie.startup().expect("startup");
            seed_query_catalog(&cassie);
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let admin_cookie = login_cookie(&client, &base_url).await;

            // Act
            let response = client
                .get(format!(
                    "{base_url}/api/v1/admin/query/schema?database=postgres"
                ))
                .header("cookie", &admin_cookie)
                .send()
                .await
                .expect("schema request");
            let status = response.status();
            let payload = response.json::<serde_json::Value>().await.expect("json");
            let section_ids = payload["sections"]
                .as_array()
                .expect("sections")
                .iter()
                .map(|section| section["id"].as_str().expect("section id"))
                .collect::<Vec<_>>();

            // Assert
            assert_eq!(
                section_ids,
                vec!["tables", "views", "indexes", "udfs", "procedures"]
            );
            assert_eq!(status, StatusCode::OK);
            assert!(contains_item(
                section_items(&payload, "tables"),
                &canonical_relation_name("postgres", "public", "rest_admin_query_docs")
            ));
            let table = section_items(&payload, "tables")
                .iter()
                .find(|item| item["name"] == "rest_admin_query_docs")
                .expect("table item");
            assert_eq!(table["database"], "postgres");
            assert_eq!(table["schema"], "public");
            assert_eq!(table["label"], "postgres.public.rest_admin_query_docs");
            assert_eq!(
                table["columns"][0]["id"],
                "column:postgres.public.rest_admin_query_docs:id"
            );
            assert_eq!(table["columns"][0]["data_type"], "int");
            assert_eq!(table["columns"][0]["primary_key"], true);
            assert_eq!(table["columns"][1]["name"], "title");
            assert_eq!(table["columns"][1]["primary_key"], false);
            assert!(contains_item(
                section_items(&payload, "views"),
                &canonical_relation_name("postgres", "public", "rest_admin_query_ready")
            ));
            assert!(contains_item(
                section_items(&payload, "indexes"),
                &canonical_relation_name("postgres", "public", "rest_admin_query_title_idx")
            ));
            assert!(contains_item(
                section_items(&payload, "udfs"),
                &canonical_relation_name("postgres", "public", "rest_query_identity")
            ));
            assert!(contains_item(
                section_items(&payload, "procedures"),
                &canonical_relation_name("postgres", "public", "rest_query_store")
            ));

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_serve_restful_admin_aliases() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("restful-aliases");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            cassie.startup().expect("startup");
            seed_query_catalog(&cassie);
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let admin_cookie = login_cookie(&client, &base_url).await;

            // Act
            let catalog_response = client
                .get(format!("{base_url}/api/v1/admin/catalog?database=postgres"))
                .header("cookie", &admin_cookie)
                .send()
                .await
                .expect("catalog request");
            let catalog_status = catalog_response.status();
            let catalog_payload = catalog_response
                .json::<serde_json::Value>()
                .await
                .expect("catalog json");
            let catalog_section_ids = catalog_payload["sections"]
                .as_array()
                .expect("sections")
                .iter()
                .map(|section| section["id"].as_str().expect("section id"))
                .collect::<Vec<_>>();

            let execution_response = post_admin_query(
                &client,
                &base_url,
                "/api/v1/admin/query-executions",
                "SELECT title FROM rest_admin_query_docs ORDER BY title",
            )
            .await;
            let execution_status = execution_response.status();
            let execution_payload = execution_response
                .json::<serde_json::Value>()
                .await
                .expect("execution json");

            let validation_response = post_admin_query(
                &client,
                &base_url,
                "/api/v1/admin/query-validations",
                "SELECT title FROM rest_admin_query_docs ORDER BY title",
            )
            .await;
            let validation_status = validation_response.status();
            let validation_payload = validation_response
                .json::<serde_json::Value>()
                .await
                .expect("validation json");

            let explain_response = post_admin_query(
                &client,
                &base_url,
                "/api/v1/admin/query-explanations",
                "SELECT title FROM rest_admin_query_docs WHERE title = 'alpha'",
            )
            .await;
            let explain_status = explain_response.status();
            let explain_payload = explain_response
                .json::<serde_json::Value>()
                .await
                .expect("explain json");

            // Assert
            assert_eq!(
                catalog_section_ids,
                vec!["tables", "views", "indexes", "udfs", "procedures"]
            );
            assert_eq!(catalog_status, StatusCode::OK);
            assert!(contains_item(
                section_items(&catalog_payload, "tables"),
                &canonical_relation_name("postgres", "public", "rest_admin_query_docs")
            ));
            assert_eq!(execution_status, StatusCode::OK);
            assert_eq!(execution_payload["command"], "SELECT");
            assert_eq!(execution_payload["rows"][0][0], "alpha");
            assert_eq!(validation_status, StatusCode::OK);
            assert_eq!(validation_payload["valid"], true);
            assert_eq!(validation_payload["command"], "SELECT");
            assert_eq!(explain_status, StatusCode::OK);
            assert_eq!(explain_payload["command"], "EXPLAIN");
            assert_eq!(explain_payload["columns"][0]["name"], "QUERY PLAN");
            assert_eq!(explain_payload["plan"]["format_version"], 1);
            assert_eq!(explain_payload["plan"]["nodes"][0]["kind"], "read");
            assert_eq!(
                explain_payload["plan"]["diagnostics"]["access_path_reason"],
                "scalar-index-seek"
            );

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }
}

// Formerly tests/rest_admin_query_method_errors.rs.
mod rest_admin_query_method_errors {
    use std::path::PathBuf;
    use std::sync::Arc;

    use cassie::app::{Cassie, CassieError};
    use reqwest::{Client, StatusCode};
    use tokio::sync::Notify;
    use uuid::Uuid;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn data_dir(label: &str) -> PathBuf {
        crate::support_temp_dirs::sweep_stale_once();
        std::env::temp_dir().join(format!(
            "cassie-rest-admin-query-{label}-{}",
            Uuid::new_v4()
        ))
    }
    async fn spawn_rest_server(
        cassie: Cassie,
    ) -> (
        String,
        Arc<Notify>,
        tokio::task::JoinHandle<Result<(), CassieError>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let shutdown = Arc::new(Notify::new());
        let server = tokio::spawn(cassie::rest::router::run_with_shutdown(
            addr.to_string(),
            cassie,
            shutdown.clone(),
        ));
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;

        (format!("http://{addr}"), shutdown, server)
    }

    async fn stop_rest_server(
        shutdown: Arc<Notify>,
        server: tokio::task::JoinHandle<Result<(), CassieError>>,
    ) {
        shutdown.notify_waiters();
        let _ = server.await;
    }

    async fn login_cookie(client: &Client, base_url: &str) -> String {
        client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "root",
                "password": "postgres"
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
            .to_string()
    }

    #[test]
    fn should_return_method_not_allowed_for_known_admin_query_paths() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("method");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            cassie.startup().expect("startup");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let admin_cookie = login_cookie(&client, &base_url).await;

            for path in [
                "/api/v1/admin/query/execute",
                "/api/v1/admin/query-executions",
            ] {
                // Act
                let response = client
                    .get(format!("{base_url}{path}"))
                    .header("cookie", &admin_cookie)
                    .send()
                    .await
                    .expect("method request");
                let status = response.status();
                let allow = response
                    .headers()
                    .get(reqwest::header::ALLOW)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                let payload = response.json::<serde_json::Value>().await.expect("json");

                // Assert
                assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
                assert_eq!(allow, "POST");
                assert_eq!(payload["error"], "method not allowed");
            }

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }
}

// Formerly tests/rest_admin_ui_static.rs.
mod rest_admin_ui_static {
    use std::path::PathBuf;
    use std::sync::Arc;

    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::Notify;
    use uuid::Uuid;

    use super::support_sql as support;
    use support::*;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn temp_path(label: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("cassie-admin-ui-{label}-{}", Uuid::new_v4()));
        path
    }

    async fn login_cookie(client: &reqwest::Client, base_url: &str) -> String {
        client
            .post(format!("{base_url}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "root",
                "password": "postgres"
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
            .to_string()
    }

    fn write_dist_fixture(label: &str) -> PathBuf {
        let dist = temp_path(label);
        std::fs::create_dir_all(dist.join("assets")).expect("create assets dir");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><html><body><div id=\"app\">Cassie Admin Shell</div></body></html>",
        )
        .expect("write index");
        std::fs::write(
            dist.join("assets").join("app.js"),
            "console.log('cassie admin asset');",
        )
        .expect("write asset");
        std::fs::write(
            dist.join("assets").join("app-AbCd1234.js"),
            "console.log('hashed cassie admin asset');",
        )
        .expect("write hashed asset");
        dist
    }

    async fn spawn_rest_server(
        cassie: Cassie,
        admin_ui_dir: PathBuf,
    ) -> (
        String,
        Arc<Notify>,
        tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let shutdown = Arc::new(Notify::new());
        let server = tokio::spawn(cassie::rest::router::run_with_shutdown_and_admin_ui_dir(
            addr.to_string(),
            cassie,
            shutdown.clone(),
            admin_ui_dir,
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        (format!("http://{addr}"), shutdown, server)
    }

    async fn stop_rest_server(
        shutdown: Arc<Notify>,
        server: tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>,
    ) {
        shutdown.notify_waiters();
        let _ = server.await;
    }

    async fn read_raw_http_response(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
        const MAX_RESPONSE_HEADER_BYTES: usize = 64 * 1024;

        let mut response = Vec::new();
        let mut chunk = [0_u8; 1024];
        let headers_end = loop {
            let read = stream.read(&mut chunk).await.expect("read HTTP response");
            assert!(read > 0, "HTTP response closed before headers completed");
            response.extend_from_slice(&chunk[..read]);
            assert!(
                response.len() <= MAX_RESPONSE_HEADER_BYTES,
                "HTTP response headers exceed test bound"
            );
            if let Some(index) = response.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = std::str::from_utf8(&response[..headers_end]).expect("HTTP response headers");
        let content_length = headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find_map(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("HTTP content length"))
            })
            .expect("HTTP response content length");
        let response_end = headers_end
            .checked_add(content_length)
            .expect("HTTP response length should fit usize");
        while response.len() < response_end {
            let read = stream
                .read(&mut chunk)
                .await
                .expect("read HTTP response body");
            assert!(read > 0, "HTTP response closed before body completed");
            response.extend_from_slice(&chunk[..read]);
        }
        response.truncate(response_end);
        response
    }

    async fn spawn_tls_rest_server(
        cassie: Cassie,
    ) -> (
        String,
        Arc<Notify>,
        tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let shutdown = Arc::new(Notify::new());
        let server = tokio::spawn(cassie::rest::router::run_with_shutdown(
            addr.to_string(),
            cassie,
            shutdown.clone(),
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        (format!("https://{addr}"), shutdown, server)
    }

    #[test]
    fn should_complete_rest_https_handshake() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("https-handshake");
        let tls_dir = temp_path("https-identity");
        std::fs::create_dir_all(&tls_dir).expect("create TLS directory");
        let certificate = tls_dir.join("cert.pem");
        let key = tls_dir.join("key.pem");
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("certificate identity");
        std::fs::write(&certificate, identity.cert.pem()).expect("certificate fixture");
        std::fs::write(&key, identity.signing_key.serialize_pem()).expect("key fixture");
        let config = CassieRuntimeConfig {
            rest_tls_cert_file: Some(certificate.to_string_lossy().to_string()),
            rest_tls_key_file: Some(key.to_string_lossy().to_string()),
            ..CassieRuntimeConfig::default()
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config).expect("cassie");

            // Act
            let (base_url, shutdown, server) = spawn_tls_rest_server(cassie).await;
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .expect("TLS client");
            let response = client
                .get(format!("{base_url}/health"))
                .send()
                .await
                .expect("HTTPS health request");

            // Assert
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            assert_eq!(
                response
                    .headers()
                    .get("strict-transport-security")
                    .and_then(|value| value.to_str().ok()),
                Some("max-age=31536000")
            );
            let login = client
                .post(format!("{base_url}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "postgres"
                }))
                .send()
                .await
                .expect("HTTPS login request");
            let cookie = login
                .headers()
                .get("set-cookie")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            assert!(cookie.contains("Secure"));
            stop_rest_server(shutdown, server).await;
        });

        let _ = std::fs::remove_dir_all(data_dir);
        let _ = std::fs::remove_dir_all(tls_dir);
    }

    #[test]
    fn should_serve_admin_index_for_shell_routes() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("admin-index");
        let dist = write_dist_fixture("admin-index");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
            let client = reqwest::Client::new();

            // Act
            let admin = client
                .get(format!("{base_url}/"))
                .send()
                .await
                .expect("admin request");
            let deep_link = client
                .get(format!("{base_url}/catalog"))
                .send()
                .await
                .expect("deep link request");
            let admin_content_type = admin
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let admin_body = admin.text().await.expect("admin body");
            let deep_link_body = deep_link.text().await.expect("deep link body");

            // Assert
            assert!(admin_content_type.contains("text/html"));
            assert!(admin_body.contains("Cassie Admin Shell"));
            assert!(deep_link_body.contains("Cassie Admin Shell"));

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }

    #[test]
    fn should_serve_built_admin_assets() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("admin-asset");
        let dist = write_dist_fixture("admin-asset");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
            let client = reqwest::Client::new();

            // Act
            let asset = client
                .get(format!("{base_url}/assets/app.js"))
                .send()
                .await
                .expect("asset request");
            let asset_status = asset.status();
            let asset_content_type = asset
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let asset_body = asset.text().await.expect("asset body");
            let hashed_asset = client
                .get(format!("{base_url}/assets/app-AbCd1234.js"))
                .send()
                .await
                .expect("hashed asset request");

            // Assert
            assert!(asset_status.is_success());
            assert!(asset_content_type.contains("javascript"));
            assert!(asset_body.contains("cassie admin asset"));
            assert_eq!(
                hashed_asset.headers()["cache-control"],
                "public, max-age=31536000, immutable"
            );

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }

    #[test]
    fn should_reject_admin_assets_over_the_static_file_limit() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("oversized-asset");
        let dist = write_dist_fixture("oversized-asset");
        let oversized = dist.join("assets").join("oversized.bin");
        std::fs::File::create(&oversized)
            .expect("oversized asset")
            .set_len(8 * 1024 * 1024 + 1)
            .expect("resize oversized asset");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;

            // Act
            let response = reqwest::get(format!("{base_url}/assets/oversized.bin"))
                .await
                .expect("oversized asset request");

            // Assert
            assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }

    #[test]
    fn should_return_not_found_when_admin_ui_dir_is_missing() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("missing-admin-ui");
        let missing_dist = temp_path("missing-admin-ui");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, missing_dist).await;
            let client = reqwest::Client::new();

            // Act
            let response = client
                .get(format!("{base_url}/"))
                .send()
                .await
                .expect("admin request");

            // Assert
            assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_reject_admin_asset_path_traversal() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("admin-traversal");
        let dist = write_dist_fixture("admin-traversal");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
            let host = base_url.strip_prefix("http://").expect("base URL host");
            let mut stream = tokio::net::TcpStream::connect(host)
                .await
                .expect("connect to rest server");
            let request = format!(
                "GET /assets/../Cargo.toml HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
            );

            // Act
            tokio::io::AsyncWriteExt::write_all(&mut stream, request.as_bytes())
                .await
                .expect("write traversal request");
            let mut response = String::new();
            tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut response)
                .await
                .expect("read traversal response");

            // Assert
            assert!(response.starts_with("HTTP/1.1 400"));
            assert!(!response.contains("[package]"));

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }

    #[test]
    fn should_preserve_existing_route_auth_behavior() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("existing-routes");
        let dist = write_dist_fixture("existing-routes");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            cassie.startup().expect("startup");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
            let client = reqwest::Client::new();

            // Act
            let health = client
                .get(format!("{base_url}/health"))
                .send()
                .await
                .expect("health request");
            let liveness = client
                .get(format!("{base_url}/liveness"))
                .send()
                .await
                .expect("liveness request");
            let targetz = client
                .get(format!("{base_url}/targetz"))
                .send()
                .await
                .expect("targetz request");
            let session_cookie = login_cookie(&client, &base_url).await;
            let metrics = client
                .get(format!("{base_url}/metrics"))
                .header("cookie", session_cookie)
                .send()
                .await
                .expect("metrics request");
            let collections = client
                .get(format!("{base_url}/api/v1/collections"))
                .send()
                .await
                .expect("collections request");

            // Assert
            assert!(health.status().is_success());
            assert!(liveness.status().is_success());
            assert!(targetz.status().is_success());
            assert!(metrics.status().is_success());
            assert_eq!(collections.status(), reqwest::StatusCode::UNAUTHORIZED);
            assert_eq!(collections.headers()["cache-control"], "no-store");
            assert_eq!(collections.headers()["x-content-type-options"], "nosniff");
            assert_eq!(collections.headers()["x-frame-options"], "DENY");
            assert_eq!(collections.headers()["referrer-policy"], "no-referrer");
            assert!(collections.headers()["content-security-policy"]
                .to_str()
                .expect("CSP header")
                .contains("default-src"));

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }

    #[test]
    fn should_reject_oversized_rest_request_body() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("oversized-body");
        let dist = write_dist_fixture("oversized-body");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
            let client = reqwest::Client::new();
            let body = vec![b'x'; 8 * 1024 * 1024 + 1];

            // Act
            let response = client
                .post(format!("{base_url}/api/v1/auth/login"))
                .header("content-type", "application/json")
                .body(body)
                .send()
                .await
                .expect("oversized request");

            // Assert
            assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }

    #[test]
    fn should_drain_declared_oversized_body_before_returning_reusable_413() {
        // Arrange
        let data_dir = data_dir("in-flight-oversized-body");
        let dist = write_dist_fixture("in-flight-oversized-body");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(
                &data_dir,
                CassieRuntimeConfig::default(),
            )
            .expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
            let address = base_url.strip_prefix("http://").expect("REST address");
            let mut stream = tokio::net::TcpStream::connect(address)
                .await
                .expect("connect raw REST client");
            let limit = 8 * 1024 * 1024;
            let declared_length = limit + 2;
            let headers = format!(
                "POST /api/v1/auth/login HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {declared_length}\r\nConnection: keep-alive\r\n\r\n"
            );
            stream
                .write_all(headers.as_bytes())
                .await
                .expect("write oversized request headers");

            // Act
            let header_only_response = tokio::time::timeout(
                std::time::Duration::from_millis(200),
                stream.read_u8(),
            )
            .await;
            stream
                .write_all(&vec![b'x'; limit + 1])
                .await
                .expect("write body through first excess byte");
            let mut peeked = [0_u8; 1];
            let before_body_complete = tokio::time::timeout(
                std::time::Duration::from_millis(200),
                stream.peek(&mut peeked),
            )
            .await;
            stream
                .write_all(b"x")
                .await
                .expect("write final declared request byte");
            let rejection = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                read_raw_http_response(&mut stream),
            )
            .await
            .expect("oversized rejection deadline");
            let health_request = format!(
                "GET /health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            );
            stream
                .write_all(health_request.as_bytes())
                .await
                .expect("reuse rejected request connection");
            let health = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                read_raw_http_response(&mut stream),
            )
            .await
            .expect("health response deadline");

            // Assert
            assert!(header_only_response.is_err());
            assert!(
                before_body_complete.is_err(),
                "server must drain the declared request before responding"
            );
            assert!(rejection.starts_with(b"HTTP/1.1 413"));
            assert!(health.starts_with(b"HTTP/1.1 200"));
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }

    #[test]
    fn should_reject_chunked_oversized_body_at_the_http_wire_limit() {
        // Arrange
        let data_dir = data_dir("chunked-oversized-body");
        let dist = write_dist_fixture("chunked-oversized-body");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(
                &data_dir,
                CassieRuntimeConfig::default(),
            )
            .expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
            let address = base_url.strip_prefix("http://").expect("REST address");
            let mut stream = tokio::net::TcpStream::connect(address)
                .await
                .expect("connect raw REST client");
            let limit = 8 * 1024 * 1024;
            let headers = format!(
                "POST /api/v1/auth/login HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{limit:X}\r\n"
            );

            // Act
            stream
                .write_all(headers.as_bytes())
                .await
                .expect("write chunked request headers");
            stream
                .write_all(&vec![b'x'; limit])
                .await
                .expect("write bounded chunk");
            stream
                .write_all(b"\r\n1\r\nx\r\n0\r\n\r\n")
                .await
                .expect("write first excess chunk");
            let rejection = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                read_raw_http_response(&mut stream),
            )
            .await
            .expect("chunked oversized rejection deadline");

            // Assert
            assert!(rejection.starts_with(b"HTTP/1.1 413"));
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }

    #[test]
    fn should_reject_non_json_rest_write_requests() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("non-json-body");
        let dist = write_dist_fixture("non-json-body");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
            // Act
            let response = reqwest::Client::new()
                .post(format!("{base_url}/api/v1/auth/login"))
                .body("username=root&password=postgres")
                .send()
                .await
                .expect("non-JSON request");

            // Assert
            assert_eq!(
                response.status(),
                reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE
            );
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }

    #[test]
    fn should_reject_cross_origin_rest_state_changes() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("cross-origin");
        let dist = write_dist_fixture("cross-origin");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
            // Act
            let response = reqwest::Client::new()
                .post(format!("{base_url}/api/v1/auth/login"))
                .header("origin", "https://evil.example")
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "postgres"
                }))
                .send()
                .await
                .expect("cross-origin request");

            // Assert
            assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }

    #[test]
    fn should_reject_ambiguous_paths_before_rest_security_checks() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("ambiguous-api-path");
        let dist = write_dist_fixture("ambiguous-api-path");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
        cassie.startup().expect("startup");
        let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
        let client = reqwest::Client::new();
        let session_cookie = login_cookie(&client, &base_url).await;

        // Act
        let body = r#"{"sql":"CREATE TABLE bypassed (id INT)"}"#;
        let address = base_url.trim_start_matches("http://");
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect raw client");
        let request = format!(
            "POST //api/v1/admin/query/execute HTTP/1.1\r\nHost: {address}\r\nOrigin: https://evil.example\r\nContent-Type: text/plain\r\nCookie: {session_cookie}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("write raw request");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("read raw response");

        // Assert
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
        stop_rest_server(shutdown, server).await;
        let _ = std::fs::remove_dir_all(data_dir);
        let _ = std::fs::remove_dir_all(dist);
    });
    }

    #[test]
    fn should_not_serve_admin_shell_for_unmatched_api_routes() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("unmatched-api");
        let dist = write_dist_fixture("unmatched-api");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            cassie.startup().expect("startup");
            let (base_url, shutdown, server) = spawn_rest_server(cassie, dist.clone()).await;
            let client = reqwest::Client::new();
            let session_cookie = login_cookie(&client, &base_url).await;

            // Act
            let response = client
                .get(format!("{base_url}/api/v1/does-not-exist"))
                .header("cookie", &session_cookie)
                .send()
                .await
                .expect("unmatched api request");
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();

            // Assert
            assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
            assert!(content_type.contains("application/json"));

            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
            let _ = std::fs::remove_dir_all(dist);
        });
    }
}

// Formerly tests/rest_embeddings.rs.
mod rest_embeddings {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::{fmt::Write as FmtWrite, path::Path};

    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::config::{
        CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig,
        SelfHostedEmbeddingRuntimeConfig,
    };
    use cassie::embeddings::openai::OpenAiConfig;
    use cassie::embeddings::DEFAULT_EMBEDDING_MODEL;
    use cassie::midge::adapter::StorageFamily;
    use cassie::rest;
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[derive(Clone)]
    struct MockResponse {
        status: u16,
        body: String,
    }

    struct MockOpenAiServer {
        base_url: String,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl MockOpenAiServer {
        fn spawn(responses: Vec<MockResponse>) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock openai");
            let base_url = format!(
                "http://{}",
                listener.local_addr().expect("mock server addr")
            );
            let thread = thread::spawn(move || {
                let responses = responses.into_iter();
                for response in responses {
                    let (mut stream, _) = listener.accept().expect("mock accept");
                    let body = read_http_body(&mut stream);
                    if body.is_empty() {
                        continue;
                    }

                    let mut output = String::new();
                    let _ = write!(output, "HTTP/1.1 {} OK\r\n", response.status);
                    output.push_str("content-type: application/json\r\n");
                    let _ = write!(output, "content-length: {}\r\n", response.body.len());
                    output.push_str("connection: close\r\n\r\n");
                    output.push_str(&response.body);
                    let _ = stream.write_all(output.as_bytes());
                    let _ = stream.flush();
                }
            });

            Self {
                base_url,
                thread: Some(thread),
            }
        }

        fn base_url(&self) -> String {
            self.base_url.clone()
        }
    }

    impl Drop for MockOpenAiServer {
        fn drop(&mut self) {
            if let Some(handle) = self.thread.take() {
                let _ = handle.join();
            }
        }
    }

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn canonical_collection(name: &str) -> String {
        canonical_relation_name("postgres", "public", name)
    }

    fn openai_runtime_with_server(base_url: String) -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.embeddings = EmbeddingsRuntimeConfig::OpenAI(OpenAiRuntimeConfig {
            config: OpenAiConfig {
                api_key: "test-key".to_string(),
                model: DEFAULT_EMBEDDING_MODEL.to_string(),
            },
            timeout_seconds: 2,
            max_batch_size: 3,
            max_retries: 1,
            base_url: Some(base_url),
        });
        config
    }

    fn tei_runtime_with_server(base_url: String) -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.embeddings = EmbeddingsRuntimeConfig::Tei(SelfHostedEmbeddingRuntimeConfig {
            base_url,
            model: "BAAI/bge-small-en-v1.5".to_string(),
            dimensions: 3,
            timeout_seconds: 2,
            max_batch_size: 3,
            max_retries: 1,
        });
        config
    }

    fn ollama_runtime_with_server(base_url: String) -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.embeddings = EmbeddingsRuntimeConfig::Ollama(SelfHostedEmbeddingRuntimeConfig {
            base_url,
            model: "nomic-embed-text".to_string(),
            dimensions: 3,
            timeout_seconds: 2,
            max_batch_size: 3,
            max_retries: 1,
        });
        config
    }

    fn response_body(vectors: &[Vec<f32>]) -> String {
        let data: Vec<_> = vectors
            .iter()
            .enumerate()
            .map(|(index, vector)| {
                serde_json::json!({
                    "index": index,
                    "embedding": vector,
                })
            })
            .collect();
        serde_json::json!({"data": data}).to_string()
    }

    fn tei_response_body(vectors: &[Vec<f32>]) -> String {
        serde_json::to_string(vectors).expect("tei response")
    }

    fn ollama_response_body(vectors: &[Vec<f32>]) -> String {
        serde_json::json!({
            "model": "nomic-embed-text",
            "embeddings": vectors,
        })
        .to_string()
    }

    fn clear_normalized_sidecars(cassie: &Cassie, collection: &str, field: &str) {
        let collection = canonical_collection(collection);
        let prefix = cassie
            .midge
            .normalized_vector_prefix_for_diagnostics(&collection, field)
            .unwrap();
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .unwrap();
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        for (key, _) in entries {
            tx.delete(key).unwrap();
        }
        tx.commit(WriteOptions::sync()).unwrap();
    }

    fn create_vector_collection(cassie: &Cassie, collection: &str, dimensions: usize) {
        rest::collections::create(
            cassie,
            serde_json::json!({
                "name": collection,
                "fields": [
                    {"name": "content", "type": "text"},
                    {"name": "label", "type": "text"},
                    {"name": "embedding", "type": format!("vector({dimensions})")},
                ],
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
    }

    fn create_vector_index(cassie: &Cassie, collection: &str, metric: &str) {
        rest::indexes::create(
            cassie,
            collection,
            serde_json::json!({
                "kind": "vector",
                "field": "embedding",
                "options": {
                    "source_field": "content",
                    "metric": metric,
                }
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
    }

    fn create_vector_index_with_options(
        cassie: &Cassie,
        collection: &str,
        options: &serde_json::Value,
    ) -> serde_json::Value {
        rest::indexes::create(
            cassie,
            collection,
            serde_json::json!({
                "kind": "vector",
                "field": "embedding",
                "options": options,
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap()
    }

    fn create_labelled_document(
        cassie: &Cassie,
        collection: &str,
        content: &str,
        label: &str,
    ) -> String {
        rest::documents::create(
            cassie,
            collection,
            serde_json::json!({"content": content, "label": label})
                .to_string()
                .as_bytes(),
        )
        .unwrap()["id"]
            .as_str()
            .expect("document id")
            .to_string()
    }

    fn vector_search(
        cassie: &Cassie,
        collection: &str,
        metric: &str,
        limit: usize,
        offset: usize,
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "field": "embedding",
            "query": "query text",
            "metric": metric,
            "limit": limit,
        });
        if offset > 0 {
            body["offset"] = serde_json::json!(offset);
        }
        serde_json::to_value(
            rest::search::vector_search(cassie, collection, body.to_string().as_bytes()).unwrap(),
        )
        .expect("search response json")
    }

    fn row_ids(search: &serde_json::Value) -> Vec<String> {
        search["rows"]
            .as_array()
            .expect("rows array")
            .iter()
            .map(|row| row[0].as_str().expect("row id").to_string())
            .collect()
    }

    fn cleanup_path(path: &Path) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn assert_normalized_fallback_metrics(
        before: &serde_json::Value,
        after_normalized: &serde_json::Value,
        after_fallback: &serde_json::Value,
    ) {
        let before_normalized = before["vector"]["normalized_candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_fallback = before["vector"]["normalized_fallback_count_total"]
            .as_u64()
            .unwrap_or_default();
        assert_eq!(
            after_normalized["vector"]["normalized_candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_normalized,
            2
        );
        assert_eq!(
            after_normalized["vector"]["normalized_fallback_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_fallback,
            0
        );
        assert_eq!(
            after_fallback["vector"]["normalized_candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - after_normalized["vector"]["normalized_candidate_count_total"]
                    .as_u64()
                    .unwrap_or_default(),
            0
        );
        assert_eq!(
            after_fallback["vector"]["normalized_fallback_count_total"]
                .as_u64()
                .unwrap_or_default()
                - after_normalized["vector"]["normalized_fallback_count_total"]
                    .as_u64()
                    .unwrap_or_default(),
            2
        );
    }

    fn search_self_hosted_vector_docs(cassie: &Cassie, collection: &str) -> Vec<String> {
        rest::collections::create(
            cassie,
            serde_json::json!({
                "name": collection,
                "fields": [
                    {"name": "content", "type": "text"},
                    {"name": "label", "type": "text"},
                    {"name": "embedding", "type": "vector(3)"},
                ],
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();

        rest::indexes::create(
            cassie,
            collection,
            serde_json::json!({
                "kind": "vector",
                "field": "embedding",
                "options": {
                    "source_field": "content",
                    "metric": "l2",
                }
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();

        let doc_one = rest::documents::create(
            cassie,
            collection,
            serde_json::json!({
                "content": "alpha",
                "label": "first",
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
        let doc_two = rest::documents::create(
            cassie,
            collection,
            serde_json::json!({
                "content": "beta",
                "label": "second",
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();

        let first_id = doc_one["id"].as_str().expect("doc one id").to_string();
        let second_id = doc_two["id"].as_str().expect("doc two id").to_string();

        let search = rest::search::vector_search(
            cassie,
            collection,
            serde_json::json!({
                "field": "embedding",
                "query": "query text",
                "metric": "l2",
                "limit": 2,
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();

        let search = serde_json::to_value(search).expect("search response json");
        let rows = search["rows"].as_array().expect("rows array");
        let returned = rows
            .iter()
            .map(|row| row[0].as_str().expect("result id").to_string())
            .collect::<Vec<_>>();
        assert_eq!(returned, vec![first_id, second_id]);
        returned
    }

    #[test]
    fn should_search_vector_docs_after_ingest() {
        // Arrange
        use_local_storage();
        let path = data_dir("search_flow");
        let path_for_cleanup = path.clone();

        let openai_server = MockOpenAiServer::spawn(vec![
            MockResponse {
                status: 200,
                body: response_body(&[{ vec![0.0; 1536] }]),
            },
            MockResponse {
                status: 200,
                body: response_body(&[{
                    let mut vector = vec![0.0; 1536];
                    vector[0] = 5.0;
                    vector
                }]),
            },
            MockResponse {
                status: 200,
                body: response_body(&[{
                    let mut vector = vec![0.0; 1536];
                    vector[0] = 2.0;
                    vector
                }]),
            },
        ]);

        let server_base_url = openai_server.base_url();
        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            openai_runtime_with_server(server_base_url),
        )
        .unwrap();

        cassie.startup().unwrap();
        create_vector_collection(&cassie, "search_collection", 1536);
        create_vector_index(&cassie, "search_collection", "l2");
        let first_id = create_labelled_document(&cassie, "search_collection", "alpha", "first");
        let second_id = create_labelled_document(&cassie, "search_collection", "beta", "second");

        // Act
        let search = vector_search(&cassie, "search_collection", "l2", 2, 0);

        // Assert
        assert_eq!(row_ids(&search), vec![first_id, second_id]);

        cleanup_path(Path::new(&path_for_cleanup));
    }

    #[test]
    fn should_search_vector_docs_with_tei_provider() {
        // Arrange
        use_local_storage();
        let path = data_dir("tei_search_flow");
        let path_for_cleanup = path.clone();

        let embedding_server = MockOpenAiServer::spawn(vec![
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![5.0, 0.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![0.0, 0.0, 0.0]]),
            },
        ]);

        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            tei_runtime_with_server(embedding_server.base_url()),
        )
        .unwrap();

        cassie.startup().unwrap();

        // Act
        let rows = search_self_hosted_vector_docs(&cassie, "tei_search_collection");
        // Assert
        assert_eq!(rows.len(), 2);

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    #[test]
    fn should_apply_vector_search_offset_after_distance_ordering() {
        // Arrange
        use_local_storage();
        let path = data_dir("vector_search_offset");
        let path_for_cleanup = path.clone();

        let embedding_server = MockOpenAiServer::spawn(vec![
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![5.0, 0.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![3.0, 0.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![0.0, 0.0, 0.0]]),
            },
        ]);

        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            tei_runtime_with_server(embedding_server.base_url()),
        )
        .unwrap();

        cassie.startup().unwrap();
        create_vector_collection(&cassie, "vector_offset_collection", 3);
        create_vector_index(&cassie, "vector_offset_collection", "l2");
        let _ = create_labelled_document(&cassie, "vector_offset_collection", "far", "third");
        let nearest_id =
            create_labelled_document(&cassie, "vector_offset_collection", "near", "first");
        let middle_id =
            create_labelled_document(&cassie, "vector_offset_collection", "middle", "second");

        // Act
        let search = vector_search(&cassie, "vector_offset_collection", "l2", 1, 1);

        // Assert
        let returned_id = row_ids(&search).into_iter().next().expect("offset row");
        assert_ne!(returned_id, nearest_id);
        assert_eq!(returned_id, middle_id);

        cleanup_path(Path::new(&path_for_cleanup));
    }

    #[test]
    fn should_fall_back_to_raw_vector_search_when_normalized_sidecars_are_missing() {
        // Arrange
        use_local_storage();
        let path = data_dir("vector_search_normalized_fallback");
        let path_for_cleanup = path.clone();

        let embedding_server = MockOpenAiServer::spawn(vec![
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![3.0, 4.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![0.0, 5.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![3.0, 4.0, 0.0]]),
            },
        ]);

        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            tei_runtime_with_server(embedding_server.base_url()),
        )
        .unwrap();

        cassie.startup().unwrap();
        create_vector_collection(&cassie, "vector_search_normalized_fallback", 3);
        create_vector_index(&cassie, "vector_search_normalized_fallback", "cosine");
        let first_id = create_labelled_document(
            &cassie,
            "vector_search_normalized_fallback",
            "alpha",
            "first",
        );
        let second_id = create_labelled_document(
            &cassie,
            "vector_search_normalized_fallback",
            "beta",
            "second",
        );

        let before = cassie.metrics();

        // Act
        let normalized_search =
            vector_search(&cassie, "vector_search_normalized_fallback", "cosine", 2, 0);
        let after_normalized = cassie.metrics();

        clear_normalized_sidecars(&cassie, "vector_search_normalized_fallback", "embedding");

        let fallback_search =
            vector_search(&cassie, "vector_search_normalized_fallback", "cosine", 2, 0);
        let after_fallback = cassie.metrics();

        // Assert
        assert_eq!(normalized_search, fallback_search);
        assert_eq!(row_ids(&normalized_search), vec![first_id, second_id]);
        assert_normalized_fallback_metrics(&before, &after_normalized, &after_fallback);

        cleanup_path(Path::new(&path_for_cleanup));
    }

    #[test]
    fn should_create_hnsw_vector_index_with_rest_option_parity() {
        // Arrange
        use_local_storage();
        let path = data_dir("rest_hnsw_options");
        let path_for_cleanup = path.clone();
        let embedding_server = MockOpenAiServer::spawn(vec![]);
        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            tei_runtime_with_server(embedding_server.base_url()),
        )
        .unwrap();
        cassie.startup().unwrap();
        create_vector_collection(&cassie, "rest_hnsw_options", 3);

        // Act
        let created = create_vector_index_with_options(
            &cassie,
            "rest_hnsw_options",
            &serde_json::json!({
                "source_field": "content",
                "metric": "l2",
                "index_type": "hnsw",
                "m": "12",
                "ef_construction": "96",
                "ef_search": "48",
            }),
        );
        let stored = cassie
            .midge
            .get_vector_index(&canonical_collection("rest_hnsw_options"), "embedding")
            .unwrap()
            .expect("stored vector index");

        // Assert
        assert_eq!(created["index_type"], "hnsw");
        assert_eq!(stored.metadata.index_type.as_str(), "hnsw");
        let hnsw = stored.metadata.hnsw.expect("hnsw options");
        assert_eq!(hnsw.m, 12);
        assert_eq!(hnsw.ef_construction, 96);
        assert_eq!(hnsw.ef_search, 48);

        cleanup_path(Path::new(&path_for_cleanup));
    }

    #[test]
    fn should_reject_invalid_rest_vector_index_options() {
        // Arrange
        use_local_storage();
        let path = data_dir("rest_invalid_hnsw_options");
        let path_for_cleanup = path.clone();
        let embedding_server = MockOpenAiServer::spawn(vec![]);
        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            tei_runtime_with_server(embedding_server.base_url()),
        )
        .unwrap();
        cassie.startup().unwrap();
        create_vector_collection(&cassie, "rest_invalid_hnsw_options", 3);

        // Act
        let error = rest::indexes::create(
            &cassie,
            "rest_invalid_hnsw_options",
            serde_json::json!({
                "kind": "vector",
                "field": "embedding",
                "options": {
                    "source_field": "content",
                    "index_type": "hnsw",
                    "m": "1",
                }
            })
            .to_string()
            .as_bytes(),
        )
        .expect_err("invalid hnsw m should fail");

        // Assert
        assert!(error
            .to_string()
            .contains("vector index option 'm' must be in [2, 128]"));

        cleanup_path(Path::new(&path_for_cleanup));
    }

    #[test]
    fn should_search_rest_hnsw_vector_index_with_graph_execution() {
        // Arrange
        use_local_storage();
        let path = data_dir("rest_hnsw_search");
        let path_for_cleanup = path.clone();
        let embedding_server = MockOpenAiServer::spawn(vec![
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![0.0, 1.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
            },
        ]);
        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            tei_runtime_with_server(embedding_server.base_url()),
        )
        .unwrap();
        cassie.startup().unwrap();
        create_vector_collection(&cassie, "rest_hnsw_search", 3);
        create_vector_index_with_options(
            &cassie,
            "rest_hnsw_search",
            &serde_json::json!({
                "source_field": "content",
                "metric": "l2",
                "index_type": "hnsw",
                "m": "2",
                "ef_construction": "4",
                "ef_search": "2",
            }),
        );
        let nearest = create_labelled_document(&cassie, "rest_hnsw_search", "near", "first");
        let _ = create_labelled_document(&cassie, "rest_hnsw_search", "far", "second");
        let before = cassie.metrics();

        // Act
        let search = vector_search(&cassie, "rest_hnsw_search", "l2", 1, 0);
        let after = cassie.metrics();

        // Assert
        assert_eq!(row_ids(&search), vec![nearest]);
        assert_eq!(
            after["vector"]["hnsw_executions"].as_u64().unwrap()
                - before["vector"]["hnsw_executions"].as_u64().unwrap(),
            1
        );

        cleanup_path(Path::new(&path_for_cleanup));
    }

    #[test]
    fn should_search_rest_ivfflat_vector_index_with_trained_candidates() {
        // Arrange
        use_local_storage();
        let path = data_dir("rest_ivfflat_search");
        let path_for_cleanup = path.clone();
        let embedding_server = MockOpenAiServer::spawn(vec![
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![0.0, 1.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: tei_response_body(&[vec![1.0, 0.0, 0.0]]),
            },
        ]);
        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            tei_runtime_with_server(embedding_server.base_url()),
        )
        .unwrap();
        cassie.startup().unwrap();
        create_vector_collection(&cassie, "rest_ivfflat_search", 3);
        let created = create_vector_index_with_options(
            &cassie,
            "rest_ivfflat_search",
            &serde_json::json!({
                "source_field": "content",
                "metric": "l2",
                "index_type": "ivfflat",
                "lists": "2",
                "probes": "1",
                "training_sample_size": "2",
                "training_seed": "7",
            }),
        );
        let nearest = create_labelled_document(&cassie, "rest_ivfflat_search", "near", "first");
        let _ = create_labelled_document(&cassie, "rest_ivfflat_search", "far", "second");
        let before = cassie.metrics();

        // Act
        let search = vector_search(&cassie, "rest_ivfflat_search", "l2", 1, 0);
        let after = cassie.metrics();

        // Assert
        assert_eq!(created["index_type"], "ivfflat");
        assert_eq!(row_ids(&search), vec![nearest]);
        assert_eq!(
            after["vector"]["ivfflat_executions"].as_u64().unwrap()
                - before["vector"]["ivfflat_executions"].as_u64().unwrap(),
            1
        );

        cleanup_path(Path::new(&path_for_cleanup));
    }

    #[test]
    fn should_search_vector_docs_with_ollama_provider() {
        // Arrange
        use_local_storage();
        let path = data_dir("ollama_search_flow");
        let path_for_cleanup = path.clone();

        let embedding_server = MockOpenAiServer::spawn(vec![
            MockResponse {
                status: 200,
                body: ollama_response_body(&[vec![1.0, 0.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: ollama_response_body(&[vec![5.0, 0.0, 0.0]]),
            },
            MockResponse {
                status: 200,
                body: ollama_response_body(&[vec![0.0, 0.0, 0.0]]),
            },
        ]);

        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            ollama_runtime_with_server(embedding_server.base_url()),
        )
        .unwrap();

        cassie.startup().unwrap();

        // Act
        let rows = search_self_hosted_vector_docs(&cassie, "ollama_search_collection");
        // Assert
        assert_eq!(rows.len(), 2);

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    #[test]
    fn should_fail_vector_search_when_metric_incompatible_with_index() {
        // Arrange
        use_local_storage();
        let path = data_dir("search_incompatible_metric");
        let path_for_cleanup = path.clone();

        let openai_server = MockOpenAiServer::spawn(vec![]);
        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            openai_runtime_with_server(openai_server.base_url()),
        )
        .unwrap();

        cassie.startup().unwrap();

        rest::collections::create(
            &cassie,
            serde_json::json!({
                "name": "search_incompatible_collection",
                "fields": [
                    {"name": "content", "type": "text"},
                    {"name": "embedding", "type": "vector(1536)"},
                ],
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();

        rest::indexes::create(
            &cassie,
            "search_incompatible_collection",
            serde_json::json!({
                "kind": "vector",
                "field": "embedding",
                "options": {
                    "source_field": "content",
                    "metric": "cosine",
                }
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();

        // Act
        let result = rest::search::vector_search(
            &cassie,
            "search_incompatible_collection",
            serde_json::json!({
                "field": "embedding",
                "query": "query text",
                "metric": "l2",
            })
            .to_string()
            .as_bytes(),
        );

        // Assert
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }

    fn read_http_body(stream: &mut std::net::TcpStream) -> Vec<u8> {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 1024];
        let mut headers_end = 0usize;
        let mut content_length = 0usize;
        while headers_end == 0 {
            let read = stream.read(&mut chunk).expect("read request");
            if read == 0 {
                return Vec::new();
            }

            buffer.extend_from_slice(&chunk[..read]);
            if let Some(separator) = find_request_body_start(&buffer) {
                headers_end = separator;
                content_length = parse_content_length(&buffer);
            }
        }

        while buffer.len() < headers_end.saturating_add(content_length) {
            let read = stream.read(&mut chunk).expect("read request body");
            if read == 0 {
                break;
            }

            buffer.extend_from_slice(&chunk[..read]);
        }

        buffer[headers_end..headers_end.saturating_add(content_length)].to_vec()
    }

    fn find_request_body_start(value: &[u8]) -> Option<usize> {
        let text = String::from_utf8_lossy(value);
        text.find("\r\n\r\n").map(|index| index + 4)
    }

    fn parse_content_length(value: &[u8]) -> usize {
        let header = String::from_utf8_lossy(value);
        for line in header.lines() {
            let lower = line.to_ascii_lowercase();
            if let Some(value) = lower.strip_prefix("content-length:") {
                if let Ok(parsed) = value.trim().parse::<usize>() {
                    return parsed;
                }
            }
        }
        0
    }
}

// Formerly tests/rest_json_contract.rs.
mod rest_json_contract {
    use cassie::app::Cassie;
    use cassie::rest::{collections, documents, health, query};
    use uuid::Uuid;

    use super::support_sql as support;
    use support::canonical_test_collection;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn data_dir(label: &str) -> String {
        crate::support_temp_dirs::sweep_stale_once();
        let mut path = std::env::temp_dir();
        path.push(format!(
            "cassie-rest-json-contract-{label}-{}",
            Uuid::new_v4()
        ));
        path.to_string_lossy().to_string()
    }

    fn setup_query_values(cassie: &Cassie) {
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_json_query_values (
                text_value TEXT,
                int_value INT,
                float_value FLOAT,
                bool_value BOOLEAN,
                null_value TEXT,
                embedding VECTOR(2),
                payload JSON
            )",
                Vec::new(),
            )
            .expect("create values table");
        let collection = canonical_test_collection(cassie, "rest_json_query_values");
        cassie
            .ingest_document(
                &collection,
                serde_json::json!({
                    "text_value": "alpha",
                    "int_value": 7,
                    "float_value": 3.5,
                    "bool_value": true,
                    "null_value": null,
                    "embedding": [1.0, 2.5],
                    "payload": {
                        "MixedCase": {
                            "innerKey": [1, true, null]
                        }
                    }
                }),
            )
            .expect("ingest values row");
    }

    fn assert_no_uppercase_response_keys(value: &serde_json::Value, skipped_prefixes: &[&str]) {
        assert_no_uppercase_response_keys_at(value, "$", skipped_prefixes);
    }

    fn assert_no_uppercase_response_keys_at(
        value: &serde_json::Value,
        path: &str,
        skipped_prefixes: &[&str],
    ) {
        if skipped_prefixes
            .iter()
            .any(|skipped_prefix| path.starts_with(skipped_prefix))
        {
            return;
        }

        match value {
            serde_json::Value::Object(object) => {
                for (key, nested) in object {
                    assert!(
                        key.chars().all(|character| !character.is_ascii_uppercase()),
                        "response key '{key}' at {path} contains uppercase characters"
                    );
                    assert_no_uppercase_response_keys_at(
                        nested,
                        format!("{path}.{key}").as_str(),
                        skipped_prefixes,
                    );
                }
            }
            serde_json::Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    assert_no_uppercase_response_keys_at(
                        item,
                        format!("{path}[{index}]").as_str(),
                        skipped_prefixes,
                    );
                }
            }
            serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_) => {}
        }
    }

    fn is_snake_case_property(name: &str) -> bool {
        !name.is_empty()
            && name.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
            })
    }

    fn line_indent(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    fn yaml_key(line: &str) -> Option<&str> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            return None;
        }
        let (key, _) = trimmed.split_once(':')?;
        Some(key.trim_matches('\'').trim_matches('"'))
    }

    fn openapi_operation<'a>(openapi: &'a str, path: &str) -> &'a str {
        let start = openapi.find(path).expect("OpenAPI path");
        let tail = &openapi[start..];
        let end = tail[1..]
            .find("\n  /")
            .map_or(tail.len(), |offset| offset + 1);
        &tail[..end]
    }

    #[test]
    fn should_serialize_admin_query_values_as_plain_json() {
        // Arrange
        use_local_storage();
        let path = data_dir("query-values");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        setup_query_values(&cassie);

        // Act
        let result = query::execute(
        &cassie,
        "postgres",
        serde_json::json!({
            "sql": "SELECT text_value, int_value, float_value, bool_value, null_value, embedding, payload FROM rest_json_query_values"
        })
        .to_string()
        .as_bytes(),
    )
    .expect("execute query");
        let payload = serde_json::to_value(result).expect("query result json");

        // Assert
        assert_eq!(payload["command"], "SELECT");
        assert_eq!(
            payload["rows"][0],
            serde_json::json!([
                "alpha",
                7,
                3.5,
                true,
                null,
                [1.0, 2.5],
                {
                    "MixedCase": {
                        "innerKey": [1, true, null]
                    }
                }
            ])
        );
        assert!(payload["rows"][0][0].get("String").is_none());
        assert!(payload["rows"][0][1].get("Int64").is_none());
        assert!(payload["rows"][0][2].get("Float64").is_none());

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_user_payload_key_shape() {
        // Arrange
        use_local_storage();
        let path = data_dir("payload-shape");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let collection = "rest_json_payload_shape";
        collections::create(
            &cassie,
            serde_json::json!({
                "name": collection,
                "fields": [
                    {"name": "CamelCase", "type": "text"},
                    {"name": "title", "type": "text"},
                    {"name": "payload", "type": "json"}
                ]
            })
            .to_string()
            .as_bytes(),
        )
        .expect("create collection");

        // Act
        let created = documents::create(
            &cassie,
            collection,
            serde_json::json!({
                "CamelCase": "kept",
                "title": "alpha",
                "payload": {
                    "MixedKey": {
                        "innerValue": 1
                    }
                }
            })
            .to_string()
            .as_bytes(),
        )
        .expect("create document");
        let id = created["id"].as_str().expect("document id");
        let loaded = documents::get(&cassie, collection, id).expect("get document");

        // Assert
        assert_eq!(loaded["CamelCase"], "kept");
        assert_eq!(loaded["payload"]["MixedKey"]["innerValue"], 1);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_store_a_value_under_the_declared_field_name_given_a_differently_cased_key() {
        // Arrange
        use_local_storage();
        let path = data_dir("payload-key-case");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let collection = "rest_json_payload_key_case";
        collections::create(
            &cassie,
            serde_json::json!({
                "name": collection,
                "fields": [{"name": "DisplayName", "type": "text"}]
            })
            .to_string()
            .as_bytes(),
        )
        .expect("create collection");

        // Act
        let created = documents::create(
            &cassie,
            collection,
            serde_json::json!({"displayname": "Alice"})
                .to_string()
                .as_bytes(),
        )
        .expect("create document");
        let id = created["id"].as_str().expect("document id");
        let loaded = documents::get(&cassie, collection, id).expect("get document");

        // Assert
        assert_eq!(loaded, serde_json::json!({"DisplayName": "Alice"}));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_enforce_unique_given_a_differently_cased_payload_key() {
        // Arrange
        use_local_storage();
        let path = data_dir("payload-key-case-unique");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_json_key_case_unique (account INT PRIMARY KEY, email TEXT UNIQUE)",
                vec![],
            )
            .expect("create table");
        let collection = "rest_json_key_case_unique";
        documents::create(
            &cassie,
            collection,
            serde_json::json!({"account": 1, "email": "a@example.com"})
                .to_string()
                .as_bytes(),
        )
        .expect("create first document");

        // Act
        let duplicate = documents::create(
            &cassie,
            collection,
            serde_json::json!({"account": 2, "EMAIL": "a@example.com"})
                .to_string()
                .as_bytes(),
        );

        // Assert
        assert!(
            matches!(
                duplicate,
                Err(cassie::app::CassieError::UniqueViolation { .. })
            ),
            "duplicate UNIQUE value with a differently cased key was accepted: {duplicate:?}"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_payload_keys_that_name_the_same_field() {
        // Arrange
        use_local_storage();
        let path = data_dir("payload-key-case-duplicate");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let collection = "rest_json_payload_key_duplicate";
        collections::create(
            &cassie,
            serde_json::json!({
                "name": collection,
                "fields": [{"name": "DisplayName", "type": "text"}]
            })
            .to_string()
            .as_bytes(),
        )
        .expect("create collection");

        // Act
        let result = documents::create(
            &cassie,
            collection,
            serde_json::json!({"DisplayName": "Alice", "displayname": "Bob"})
                .to_string()
                .as_bytes(),
        );

        // Assert
        let error = result.expect_err("ambiguous payload keys must be rejected");
        assert!(
            matches!(&error, cassie::app::CassieError::InvalidVector(_)),
            "unexpected error variant: {error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("field 'DisplayName' is specified more than once"),
            "unexpected error: {error}"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_metadata_response_keys_snake_case() {
        // Arrange
        use_local_storage();
        let path = data_dir("response-keys");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        setup_query_values(&cassie);
        let collection_response = collections::create(
            &cassie,
            serde_json::json!({
                "name": "rest_json_contract_docs",
                "fields": [{"name": "title", "type": "text"}]
            })
            .to_string()
            .as_bytes(),
        )
        .expect("create collection");

        // Act
        let responses = vec![
            health::liveness(&cassie),
            health::health(&cassie),
            collection_response,
            serde_json::to_value(query::schema(&cassie)).expect("schema json"),
            serde_json::to_value(
                query::validate(
                    &cassie,
                    serde_json::json!({"sql": "SELECT text_value FROM rest_json_query_values"})
                        .to_string()
                        .as_bytes(),
                )
                .expect("validate query"),
            )
            .expect("validate json"),
            serde_json::to_value(
                query::execute(
                    &cassie,
                    "postgres",
                    serde_json::json!({"sql": "SELECT payload FROM rest_json_query_values"})
                        .to_string()
                        .as_bytes(),
                )
                .expect("execute query"),
            )
            .expect("execute json"),
        ];

        // Assert
        for response in responses {
            assert_no_uppercase_response_keys(&response, &["$.rows"]);
        }

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_document_rest_transport_policy_contract() {
        // Arrange
        let openapi = std::fs::read_to_string("public/openapi.yml").expect("openapi document");

        // Act
        let describes_limits = openapi.contains("8 MiB")
            && openapi.contains("32 KiB")
            && openapi.contains("10 seconds")
            && openapi.contains("30 seconds");
        let describes_browser_policy = openapi.contains("same-origin")
            && openapi.contains("cross-origin")
            && openapi.contains("HSTS");
        let describes_statuses = openapi.contains("RequestTimeout")
            && openapi.contains("RequestEntityTooLarge")
            && openapi.contains("UnsupportedMediaType");

        // Assert
        assert!(
            describes_limits,
            "OpenAPI must document enforced REST limits"
        );
        assert!(
            describes_browser_policy,
            "OpenAPI must document same-origin and TLS browser policy"
        );
        assert!(
            describes_statuses,
            "OpenAPI must name deterministic transport error responses"
        );
    }

    #[test]
    fn should_keep_openapi_payload_properties_snake_case() {
        // Arrange
        let openapi = std::fs::read_to_string("public/openapi.yml").expect("openapi");
        let mut properties_indent = None;
        let openapi_lines = openapi.lines().enumerate();

        // Act
        for (line_index, line) in openapi_lines {
            let indent = line_indent(line);
            if properties_indent.is_some_and(|active_indent| indent <= active_indent) {
                properties_indent = None;
            }

            if line.trim() == "properties:" {
                properties_indent = Some(indent);
                continue;
            }

            let Some(active_indent) = properties_indent else {
                continue;
            };
            if indent != active_indent + 2 {
                continue;
            }
            let Some(property) = yaml_key(line) else {
                continue;
            };

            // Assert
            assert!(
                is_snake_case_property(property),
                "OpenAPI payload property '{property}' on line {} is not snake_case",
                line_index + 1
            );
        }
    }

    #[test]
    fn should_document_restful_catalog_contract_in_openapi() {
        // Arrange
        let openapi = std::fs::read_to_string("public/openapi.yml").expect("openapi");

        // Act
        let expected_entries = [
        "/api/v1/admin/catalog:",
        "/api/v1/admin/query-executions:",
        "/api/v1/admin/query-validations:",
        "/api/v1/admin/query-explanations:",
        "/api/v1/admin/projections/{projection}/verification-manifests:",
        "QueryExplainResponse:",
        "Structured physical-plan summary intended for visual plan explorers.",
        "Explain output with text rows and structured plan data",
        "The REST API does not expose first-class `/api/v1/databases` or `/api/v1/schemas` resources.",
        "Use the authenticated SQL administration endpoints for supported database, schema, table, view, function, and procedure statements.",
        "The router accepts local names, schema-qualified names, or canonical `database.schema.name` identifiers.",
        "Labels use canonical `database.schema.name` values when scope is available.",
    ];

        // Assert
        for expected in expected_entries {
            assert!(
                openapi.contains(expected),
                "expected OpenAPI contract to contain: {expected}"
            );
        }
    }

    #[test]
    fn should_document_admin_query_runtime_errors() {
        // Arrange
        let openapi = std::fs::read_to_string("public/openapi.yml").expect("openapi");
        let query_paths = [
            "/api/v1/admin/query-executions:",
            "/api/v1/admin/query/execute:",
            "/api/v1/admin/query-validations:",
            "/api/v1/admin/query/validate:",
            "/api/v1/admin/query-explanations:",
            "/api/v1/admin/query/explain:",
        ];
        let runtime_statuses = [
            "'404':", "'408':", "'409':", "'413':", "'415':", "'499':", "'501':", "'503':",
            "'504':",
        ];

        // Act
        let operations = query_paths.map(|path| (path, openapi_operation(&openapi, path)));

        // Assert
        for (path, operation) in operations {
            for status in runtime_statuses {
                assert!(
                    operation.contains(status),
                    "{path} must document runtime status {status}"
                );
            }
        }
    }

    #[test]
    fn should_document_logout_response_payload() {
        // Arrange
        let openapi = std::fs::read_to_string("public/openapi.yml").expect("openapi");

        // Act
        let logout = openapi_operation(&openapi, "/api/v1/auth/logout:");

        // Assert
        assert!(logout.contains("$ref: '#/components/schemas/LogoutResponse'"));
        assert!(openapi.contains("LogoutResponse:"));
        assert!(openapi.contains("logged_out:"));
    }

    #[test]
    fn should_document_admin_ui_authentication_boundary_errors() {
        // Arrange
        let openapi = std::fs::read_to_string("public/openapi.yml").expect("openapi");
        let expected_statuses = [
            (
                "/api/v1/auth/login:",
                &[
                    "'400':", "'401':", "'403':", "'404':", "'408':", "'413':", "'415':", "'501':",
                    "'503':",
                ] as &[&str],
            ),
            ("/api/v1/auth/session:", &["'401':", "'408':", "'503':"]),
            (
                "/api/v1/admin/catalog:",
                &["'401':", "'403':", "'405':", "'408':", "'500':", "'503':"],
            ),
            (
                "/api/v1/admin/query/schema:",
                &["'401':", "'403':", "'405':", "'408':", "'500':", "'503':"],
            ),
        ];

        // Act
        let operations = expected_statuses
            .map(|(path, statuses)| (path, statuses, openapi_operation(&openapi, path)));

        // Assert
        for (path, statuses, operation) in operations {
            for status in statuses {
                assert!(
                    operation.contains(status),
                    "{path} must document runtime status {status}"
                );
            }
        }
    }
}

// Formerly tests/rest_metrics.rs.
mod rest_metrics {
    use cassie::app::Cassie;

    use super::support_sql as support;
    use support::*;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    async fn login_cookie(client: &reqwest::Client, addr: std::net::SocketAddr) -> String {
        client
            .post(format!("http://{addr}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "root",
                "password": "postgres"
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
            .to_string()
    }

    fn assert_rest_metrics(metrics: &serde_json::Value) {
        assert!(
            metrics["rest"]["requests_total"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );
        assert!(
            metrics["rest"]["by_method"]["GET"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );
        assert!(
            metrics["rest"]["by_route"]["/health"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );
        assert!(
            metrics["rest"]["by_route"]["/liveness"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );
        assert!(
            metrics["rest"]["by_status_class"]["2xx"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );
        let routes = metrics["rest"]["by_route"]
            .as_object()
            .expect("route metrics");
        assert!(routes.len() <= 257);
        assert!(routes["<other>"].as_u64().unwrap_or_default() > 0);
    }

    #[test]
    fn should_record_rest_request_metrics_for_http_routes() {
        // Arrange
        use_local_storage();
        let path = data_dir("http_routes");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);

            let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie.clone()));

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let client = reqwest::Client::new();

            // Act
            let health = client
                .get(format!("http://{addr}/health"))
                .send()
                .await
                .expect("health request");
            assert!(health.status().is_success());

            let liveness = client
                .get(format!("http://{addr}/liveness"))
                .send()
                .await
                .expect("liveness request");
            assert!(liveness.status().is_success());
            for index in 0..300 {
                client
                    .get(format!("http://{addr}/unmatched-{index}"))
                    .send()
                    .await
                    .expect("unmatched request");
            }

            let unauthenticated_metrics = client
                .get(format!("http://{addr}/metrics"))
                .send()
                .await
                .expect("unauthenticated metrics request");
            let cookie = login_cookie(&client, addr).await;
            let metrics = client
                .get(format!("http://{addr}/metrics"))
                .header("cookie", cookie)
                .send()
                .await
                .expect("authenticated metrics request")
                .json::<serde_json::Value>()
                .await
                .expect("metrics json");

            // Assert
            assert_eq!(
                unauthenticated_metrics.status(),
                reqwest::StatusCode::UNAUTHORIZED
            );
            assert_rest_metrics(&metrics);

            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/rest_sessions.rs.
mod rest_sessions {
    use std::path::PathBuf;
    use std::sync::Arc;

    use cassie::app::{Cassie, CassieError};
    use cassie::config::CassieRuntimeConfig;
    use reqwest::{Client, StatusCode};
    use tokio::sync::Notify;
    use uuid::Uuid;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn data_dir(label: &str) -> PathBuf {
        crate::support_temp_dirs::sweep_stale_once();
        std::env::temp_dir().join(format!("cassie-rest-sessions-{label}-{}", Uuid::new_v4()))
    }

    async fn spawn_rest_server(
        cassie: Cassie,
    ) -> (
        String,
        Arc<Notify>,
        tokio::task::JoinHandle<Result<(), CassieError>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let shutdown = Arc::new(Notify::new());
        let server = tokio::spawn(cassie::rest::router::run_with_shutdown(
            address.to_string(),
            cassie,
            shutdown.clone(),
        ));
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
        (format!("http://{address}"), shutdown, server)
    }

    async fn stop_rest_server(
        shutdown: Arc<Notify>,
        server: tokio::task::JoinHandle<Result<(), CassieError>>,
    ) {
        shutdown.notify_waiters();
        let _ = server.await;
    }

    fn cookie(response: &reqwest::Response) -> String {
        response
            .headers()
            .get("set-cookie")
            .expect("session cookie")
            .to_str()
            .expect("cookie header")
            .split(';')
            .next()
            .expect("cookie pair")
            .to_string()
    }

    #[test]
    fn should_reject_invalid_rest_login_without_issuing_a_cookie() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("invalid-login");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();

            // Act
            let response = client
                .post(format!("{base_url}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "wrong"
                }))
                .send()
                .await
                .expect("login response");

            // Assert
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert!(response.headers().get("set-cookie").is_none());
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_revoke_cookie_session_after_logout() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("session-lifecycle");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let login = client
                .post(format!("{base_url}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "postgres"
                }))
                .send()
                .await
                .expect("login response");
            let session_cookie = cookie(&login);
            let set_cookie = login
                .headers()
                .get("set-cookie")
                .expect("set-cookie")
                .to_str()
                .expect("set-cookie value");

            // Act
            let current = client
                .get(format!("{base_url}/api/v1/auth/session"))
                .header("cookie", &session_cookie)
                .send()
                .await
                .expect("current session response");
            let logout = client
                .post(format!("{base_url}/api/v1/auth/logout"))
                .header("cookie", &session_cookie)
                .send()
                .await
                .expect("logout response");
            let after_logout = client
                .get(format!("{base_url}/api/v1/auth/session"))
                .header("cookie", &session_cookie)
                .send()
                .await
                .expect("post-logout response");

            // Assert
            assert_eq!(login.status(), StatusCode::OK);
            assert!(set_cookie.contains("HttpOnly"));
            assert!(set_cookie.contains("SameSite=Strict"));
            assert_eq!(current.status(), StatusCode::OK);
            assert_eq!(logout.status(), StatusCode::OK);
            assert!(logout
                .headers()
                .get("set-cookie")
                .expect("clear cookie")
                .to_str()
                .expect("clear cookie value")
                .contains("Max-Age=0"));
            assert_eq!(after_logout.status(), StatusCode::UNAUTHORIZED);
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_reject_password_bearing_bearer_credentials() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("bearer-rejected");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();

            // Act
            let response = client
                .get(format!("{base_url}/api/v1/auth/session"))
                .header("authorization", "Bearer root:postgres")
                .send()
                .await
                .expect("bearer response");

            // Assert
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_ignore_forwarding_headers_when_deriving_plaintext_cookie_security() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("forwarded-headers");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();

            // Act
            let response = client
                .post(format!("{base_url}/api/v1/auth/login"))
                .header("forwarded", "proto=https")
                .header("x-forwarded-proto", "https")
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "postgres"
                }))
                .send()
                .await
                .expect("login response");

            // Assert
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!response
                .headers()
                .get("set-cookie")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("Secure")));
            assert!(!response.headers().contains_key("strict-transport-security"));
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_secure_external_https_login_logout_cookies() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("external-https");
        let config = CassieRuntimeConfig {
            rest_external_https: true,
            ..CassieRuntimeConfig::default()
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&data_dir, config).expect("configured cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let login = client
                .post(format!("{base_url}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "postgres"
                }))
                .send()
                .await
                .expect("login response");
            let session_cookie = cookie(&login);

            // Act
            let logout = client
                .post(format!("{base_url}/api/v1/auth/logout"))
                .header("cookie", session_cookie)
                .send()
                .await
                .expect("logout response");

            // Assert
            assert!(login
                .headers()
                .get("set-cookie")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("Secure")));
            assert!(login.headers().contains_key("strict-transport-security"));
            assert!(logout
                .headers()
                .get("set-cookie")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("Max-Age=0") && value.contains("Secure")));
            assert!(logout.headers().contains_key("strict-transport-security"));
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_refund_success_before_throttling_excess_rest_logins() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("login-throttle");
        let config = CassieRuntimeConfig {
            auth_user_attempts_per_minute: 1,
            auth_ip_attempts_per_minute: 10,
            ..CassieRuntimeConfig::default()
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Cassie::new_with_data_dir_and_config(&data_dir, config).expect("configured cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let valid_login = || {
                client
                    .post(format!("{base_url}/api/v1/auth/login"))
                    .json(&serde_json::json!({
                        "username": "root",
                        "password": "postgres"
                    }))
                    .send()
            };

            // Act
            let first_success = valid_login().await.expect("first success");
            let second_success = valid_login().await.expect("refunded success");
            let invalid = client
                .post(format!("{base_url}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "wrong"
                }))
                .send()
                .await
                .expect("invalid login");
            let throttled = client
                .post(format!("{base_url}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "wrong"
                }))
                .send()
                .await
                .expect("throttled login");

            // Assert
            assert_eq!(first_success.status(), StatusCode::OK);
            assert_eq!(second_success.status(), StatusCode::OK);
            assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(
                throttled
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok()),
                Some("60")
            );
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }

    #[test]
    fn should_map_oversized_rest_sql_to_bad_request() {
        // Arrange
        use_local_storage();
        let data_dir = data_dir("sql-resource-limit");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&data_dir).expect("cassie");
            let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
            let client = Client::new();
            let login = client
                .post(format!("{base_url}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "postgres"
                }))
                .send()
                .await
                .expect("login response");
            let session_cookie = cookie(&login);

            // Act
            let response = client
                .post(format!("{base_url}/api/v1/admin/query-executions"))
                .header("cookie", session_cookie)
                .json(&serde_json::json!({
                    "database": "postgres",
                    "sql": "x".repeat(1024 * 1024 + 1)
                }))
                .send()
                .await
                .expect("query response");
            let status = response.status();
            let body = response.text().await.expect("error body");

            // Assert
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(body.contains("SQL text exceeds"));
            stop_rest_server(shutdown, server).await;
            let _ = std::fs::remove_dir_all(data_dir);
        });
    }
}
