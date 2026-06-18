use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;
use uuid::Uuid;

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-auth-{label}-{}", Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

#[test]
fn should_default_new_session_database_from_config() {
    // Arrange
    let path = data_dir("session_default_db");
    let config = CassieRuntimeConfig {
        database: "tenant_db".to_string(),
        user: "admin".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let session = cassie.create_session("tester", None).await;

        // Assert
        assert_eq!(session.database, Some("tenant_db".to_string()));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_expose_session_identity_in_context_functions() {
    // Arrange
    let path = data_dir("context_functions");
    let config = CassieRuntimeConfig {
        database: "postgres".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("alice", None).await;
        let functions = [
            "current_user()",
            "session_user()",
            "current_role()",
            "current_database()",
        ];
        let mut actual = Vec::new();

        for function in functions {
            // Act
            let query = format!("SELECT {function}");
            let result = cassie
                .execute_sql(&session, &query, vec![])
                .await
                .expect("identity function query");
            let value = result
                .rows
                .first()
                .and_then(|row| row.first())
                .cloned()
                .expect("row present");
            actual.push(value);
        }

        // Assert
        assert_eq!(actual[0], Value::String("alice".to_string()));
        assert_eq!(actual[1], Value::String("alice".to_string()));
        assert_eq!(actual[2], Value::String("alice".to_string()));
        assert_eq!(actual[3], Value::String("postgres".to_string()));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_present_default_admin_role_in_pg_roles() {
    // Arrange
    let path = data_dir("pg_roles");
    let config = CassieRuntimeConfig {
        user: "admin".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.startup().await.unwrap();

        // Act
        let session = cassie.create_session("alice", None).await;
        let result = cassie
            .execute_sql(
                &session,
                "SELECT rolname FROM pg_catalog.pg_roles ORDER BY rolname",
                vec![],
            )
            .await
            .expect("pg_roles query");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("admin".to_string())]]);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_persist_created_login_role_in_pg_roles() {
    // Arrange
    let path = data_dir("create_login_role");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.startup().await.unwrap();
        let admin = cassie
            .authenticate_role("sa", Some("sa-secret"), None)
            .await
            .expect("admin login");

        // Act
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                vec![],
            )
            .await
            .expect("create role");
        let result = cassie
            .execute_sql(
                &admin,
                "SELECT rolname FROM pg_catalog.pg_roles ORDER BY rolname",
                vec![],
            )
            .await
            .expect("pg_roles query");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("alice".to_string())],
                vec![Value::String("sa".to_string())],
            ]
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_authenticate_persisted_login_role_with_password() {
    // Arrange
    let path = data_dir("login_role_auth");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.startup().await.unwrap();
        let admin = cassie
            .authenticate_role("sa", Some("sa-secret"), None)
            .await
            .expect("admin login");
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                vec![],
            )
            .await
            .expect("create role");

        // Act
        let alice = cassie
            .authenticate_role("alice", Some("alice-secret"), None)
            .await
            .expect("alice login");
        let result = cassie
            .execute_sql(
                &alice,
                "SELECT current_user(), session_user(), current_role(), current_database()",
                vec![],
            )
            .await
            .expect("identity query");

        // Assert
        assert_eq!(alice.user, "alice");
        assert_eq!(
            result.rows,
            vec![vec![
                Value::String("alice".to_string()),
                Value::String("alice".to_string()),
                Value::String("alice".to_string()),
                Value::String("postgres".to_string()),
            ]]
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rotate_login_role_password() {
    // Arrange
    let path = data_dir("rotate_login_role_password");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.startup().await.unwrap();
        let admin = cassie
            .authenticate_role("sa", Some("sa-secret"), None)
            .await
            .expect("admin login");
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                vec![],
            )
            .await
            .expect("create role");

        // Act
        cassie
            .execute_sql(&admin, "ALTER ROLE alice PASSWORD 'alice-rotated'", vec![])
            .await
            .expect("rotate password");

        let old_password = cassie
            .authenticate_role("alice", Some("alice-secret"), None)
            .await;
        let new_password = cassie
            .authenticate_role("alice", Some("alice-rotated"), None)
            .await;

        // Assert
        assert!(old_password.is_err(), "old password should be rejected");
        assert!(new_password.is_ok(), "new password should be accepted");
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_drop_login_role() {
    // Arrange
    let path = data_dir("drop_login_role");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.startup().await.unwrap();
        let admin = cassie
            .authenticate_role("sa", Some("sa-secret"), None)
            .await
            .expect("admin login");
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                vec![],
            )
            .await
            .expect("create role");

        // Act
        cassie
            .execute_sql(&admin, "DROP ROLE alice", vec![])
            .await
            .expect("drop role");

        // Assert
        let roles = cassie
            .execute_sql(
                &admin,
                "SELECT rolname FROM pg_catalog.pg_roles ORDER BY rolname",
                vec![],
            )
            .await
            .expect("pg_roles query");

        assert_eq!(
            roles.rows,
            vec![vec![Value::String("sa".to_string())]],
            "dropped role should be removed from the catalog"
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_authentication_for_dropped_login_role() {
    // Arrange
    let path = data_dir("drop_login_role_auth");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "sa-secret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.startup().await.unwrap();
        let admin = cassie
            .authenticate_role("sa", Some("sa-secret"), None)
            .await
            .expect("admin login");
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                vec![],
            )
            .await
            .expect("create role");
        cassie
            .execute_sql(&admin, "DROP ROLE alice", vec![])
            .await
            .expect("drop role");

        // Act
        let result = cassie
            .authenticate_role("alice", Some("alice-secret"), None)
            .await;

        // Assert
        assert!(result.is_err(), "dropped role should not authenticate");
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_enforce_deterministic_rest_bearer_auth() {
    // Arrange
    let path = data_dir("rest_auth");
    let config = CassieRuntimeConfig {
        user: "sa".to_string(),
        password: "topsecret".to_string(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.startup().await.unwrap();
        let admin = cassie
            .authenticate_role("sa", Some("topsecret"), None)
            .await
            .expect("admin login");
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                vec![],
            )
            .await
            .expect("create role");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie.clone()));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let client = reqwest::Client::new();

        // Act
        let unauthorized = client
            .get(format!("http://{addr}/v1/collections"))
            .send()
            .await
            .expect("request with no auth");

        let wrong_token = client
            .get(format!("http://{addr}/v1/collections"))
            .header("authorization", "Bearer sa:wrong-token")
            .send()
            .await
            .expect("request with wrong auth");

        let authorized = client
            .get(format!("http://{addr}/v1/collections"))
            .header("authorization", "Bearer sa:topsecret")
            .send()
            .await
            .expect("request with correct auth");

        let forbidden = client
            .get(format!("http://{addr}/v1/collections"))
            .header("authorization", "Bearer alice:alice-secret")
            .send()
            .await
            .expect("request with non-admin auth");

        let health = client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .expect("health request");

        let metrics = client
            .get(format!("http://{addr}/metrics"))
            .send()
            .await
            .expect("metrics request");

        // Assert
        assert!(unauthorized.status() == reqwest::StatusCode::UNAUTHORIZED);
        assert!(wrong_token.status() == reqwest::StatusCode::UNAUTHORIZED);
        assert!(authorized.status().is_success());
        assert!(forbidden.status() == reqwest::StatusCode::FORBIDDEN);
        assert!(health.status().is_success());
        assert!(metrics.status().is_success());

        server.abort();
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
}
