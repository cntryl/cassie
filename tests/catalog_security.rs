// Consolidated integration suite: catalog_security.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/catalog.rs"]
mod support_catalog;
#[path = "support/data_dir.rs"]
mod support_data_dir;
#[path = "support/executor.rs"]
mod support_executor;
#[path = "support/local_storage.rs"]
mod support_local_storage;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/auth.rs.
mod auth {
    use super::support_sql as support;
    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::Value;
    use support::*;

    async fn rest_login_cookie(
        client: &reqwest::Client,
        url: String,
        user: &str,
        password: &str,
    ) -> String {
        client
            .post(url)
            .json(&serde_json::json!({
                "username": user,
                "password": password
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
    fn should_default_new_session_database_from_config() {
        // Arrange
        let path = data_dir("session_default_db");
        let config = CassieRuntimeConfig {
            database: "tenant_db".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Act
            let session = cassie.create_session("tester", None);

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
            let session = cassie.create_session("alice", None);
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
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.startup().unwrap();

            // Act
            let session = cassie.create_session("alice", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT rolname FROM pg_catalog.pg_roles ORDER BY rolname",
                    vec![],
                )
                .expect("pg_roles query");

            // Assert
            assert_eq!(result.rows, vec![vec![Value::String("root".to_string())]]);
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_persist_created_login_role_in_pg_roles() {
        // Arrange
        let path = data_dir("create_login_role");
        let config = CassieRuntimeConfig {
            password: "sa-secret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.startup().unwrap();
            let admin = cassie
                .authenticate_role("root", Some("sa-secret"), None)
                .expect("admin login");

            // Act
            cassie
                .execute_sql(
                    &admin,
                    "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                    vec![],
                )
                .expect("create role");
            let result = cassie
                .execute_sql(
                    &admin,
                    "SELECT rolname FROM pg_catalog.pg_roles ORDER BY rolname",
                    vec![],
                )
                .expect("pg_roles query");

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::String("alice".to_string())],
                    vec![Value::String("root".to_string())],
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
            password: "sa-secret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.startup().unwrap();
            let admin = cassie
                .authenticate_role("root", Some("sa-secret"), None)
                .expect("admin login");
            cassie
                .execute_sql(
                    &admin,
                    "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                    vec![],
                )
                .expect("create role");

            // Act
            let alice = cassie
                .authenticate_role("alice", Some("alice-secret"), None)
                .expect("alice login");
            let result = cassie
                .execute_sql(
                    &alice,
                    "SELECT current_user(), session_user(), current_role(), current_database()",
                    vec![],
                )
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
            password: "sa-secret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.startup().unwrap();
            let admin = cassie
                .authenticate_role("root", Some("sa-secret"), None)
                .expect("admin login");
            cassie
                .execute_sql(
                    &admin,
                    "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                    vec![],
                )
                .expect("create role");

            // Act
            cassie
                .execute_sql(&admin, "ALTER ROLE alice PASSWORD 'alice-rotated'", vec![])
                .expect("rotate password");

            let old_password = cassie.authenticate_role("alice", Some("alice-secret"), None);
            let new_password = cassie.authenticate_role("alice", Some("alice-rotated"), None);

            // Assert
            assert!(old_password.is_err(), "old password should be rejected");
            assert!(new_password.is_ok(), "new password should be accepted");
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_require_an_explicit_database_grant_for_non_admin_roles() {
        // Arrange
        let path = data_dir("database_grants");
        let config = CassieRuntimeConfig {
            password: "sa-secret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let admin = cassie
            .authenticate_role("root", Some("sa-secret"), None)
            .expect("admin login");
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                vec![],
            )
            .expect("create role");
        cassie
            .midge
            .create_database("analytics", None)
            .expect("analytics database");

        // Act
        let before_grant =
            cassie.authenticate_role("alice", Some("alice-secret"), Some("analytics".to_string()));
        cassie
            .grant_role_database_access(&admin, "alice", "analytics")
            .expect("grant analytics");
        let after_grant =
            cassie.authenticate_role("alice", Some("alice-secret"), Some("analytics".to_string()));
        cassie
            .revoke_role_database_access(&admin, "alice", "analytics")
            .expect("revoke analytics");
        let after_revoke =
            cassie.authenticate_role("alice", Some("alice-secret"), Some("analytics".to_string()));

        // Assert
        assert!(matches!(
            before_grant,
            Err(cassie::app::CassieError::InsufficientPrivilege)
        ));
        assert!(after_grant.is_ok());
        assert!(matches!(
            after_revoke,
            Err(cassie::app::CassieError::InsufficientPrivilege)
        ));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_drop_login_role() {
        // Arrange
        let path = data_dir("drop_login_role");
        let config = CassieRuntimeConfig {
            password: "sa-secret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.startup().unwrap();
            let admin = cassie
                .authenticate_role("root", Some("sa-secret"), None)
                .expect("admin login");
            cassie
                .execute_sql(
                    &admin,
                    "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                    vec![],
                )
                .expect("create role");

            // Act
            cassie
                .execute_sql(&admin, "DROP ROLE alice", vec![])
                .expect("drop role");

            // Assert
            let roles = cassie
                .execute_sql(
                    &admin,
                    "SELECT rolname FROM pg_catalog.pg_roles ORDER BY rolname",
                    vec![],
                )
                .expect("pg_roles query");

            assert_eq!(
                roles.rows,
                vec![vec![Value::String("root".to_string())]],
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
            password: "sa-secret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.startup().unwrap();
            let admin = cassie
                .authenticate_role("root", Some("sa-secret"), None)
                .expect("admin login");
            cassie
                .execute_sql(
                    &admin,
                    "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                    vec![],
                )
                .expect("create role");
            cassie
                .execute_sql(&admin, "DROP ROLE alice", vec![])
                .expect("drop role");

            // Act
            let result = cassie.authenticate_role("alice", Some("alice-secret"), None);

            // Assert
            assert!(result.is_err(), "dropped role should not authenticate");
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_require_opaque_rest_sessions() {
        // Arrange
        let path = data_dir("rest_auth");
        let config = CassieRuntimeConfig {
            password: "topsecret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.startup().unwrap();
            let admin = cassie
                .authenticate_role("root", Some("topsecret"), None)
                .expect("admin login");
            cassie
                .execute_sql(
                    &admin,
                    "CREATE ROLE alice LOGIN PASSWORD 'alice-secret'",
                    vec![],
                )
                .expect("create role");

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);

            let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie.clone()));
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let client = reqwest::Client::new();

            let admin_cookie = rest_login_cookie(
                &client,
                format!("http://{addr}/api/v1/auth/login"),
                "root",
                "topsecret",
            )
            .await;
            let reader_cookie = rest_login_cookie(
                &client,
                format!("http://{addr}/api/v1/auth/login"),
                "alice",
                "alice-secret",
            )
            .await;

            // Act
            let unauthorized = client
                .get(format!("http://{addr}/api/v1/collections"))
                .send()
                .await
                .expect("request with no auth");

            let wrong_token = client
                .get(format!("http://{addr}/api/v1/collections"))
                .header("authorization", "Bearer sa:wrong-token")
                .send()
                .await
                .expect("request with wrong auth");

            let authorized = client
                .get(format!("http://{addr}/api/v1/collections"))
                .header("cookie", &admin_cookie)
                .send()
                .await
                .expect("request with correct auth");

            let forbidden = client
                .get(format!("http://{addr}/api/v1/collections"))
                .header("cookie", &reader_cookie)
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
            assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
            assert_eq!(wrong_token.status(), reqwest::StatusCode::UNAUTHORIZED);
            assert!(authorized.status().is_success());
            assert_eq!(forbidden.status(), reqwest::StatusCode::FORBIDDEN);
            assert!(health.status().is_success());
            assert_eq!(metrics.status(), reqwest::StatusCode::UNAUTHORIZED);

            server.abort();
            let _ = server.await;
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/bootstrap_credential_rotation.rs.
mod bootstrap_credential_rotation {
    use super::support_sql as support;
    use cassie::app::{Cassie, CassieError};
    use cassie::config::CassieRuntimeConfig;
    use support::*;

    fn config(password: &str) -> CassieRuntimeConfig {
        CassieRuntimeConfig {
            password: password.to_string(),
            ..CassieRuntimeConfig::default()
        }
    }

    #[test]
    fn should_make_the_configured_bootstrap_password_authoritative_after_restart() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("restart");
        {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config("old-secret"))
                .expect("initial cassie");
            cassie.startup().expect("initial startup");
            cassie.shutdown();
        }

        // Act
        let restarted = Cassie::new_with_data_dir_and_config(&path, config("new-secret"))
            .expect("restarted cassie");
        restarted.startup().expect("restarted startup");
        let old_password = restarted.authenticate_role("root", Some("old-secret"), None);
        let new_password = restarted.authenticate_role("root", Some("new-secret"), None);

        // Assert
        assert!(matches!(old_password, Err(CassieError::Unauthorized)));
        assert!(new_password.is_ok());
        restarted.shutdown();
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/catalog_introspection.rs.
mod catalog_introspection {
    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::{DataType, Value};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn use_local_storage() {
        if std::env::var("CASSIE_EMBEDDINGS_PROVIDER").is_err() {
            std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "fallback");
        }
    }

    fn data_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cassie-catalog-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn should_list_user_tables_through_information_schema() {
        // Arrange
        use_local_storage();
        let path = data_dir("tables");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(
                &path,
                cassie::config::CassieRuntimeConfig {
                    ..cassie::config::CassieRuntimeConfig::default()
                },
            )
            .unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE catalog_tables_docs (title TEXT)",
                    vec![],
                )

    .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT table_name FROM information_schema.tables WHERE table_name = 'catalog_tables_docs'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("catalog_tables_docs".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_list_columns_through_information_schema_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("columns_restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(
                &path,
                cassie::config::CassieRuntimeConfig {
                    ..cassie::config::CassieRuntimeConfig::default()
                },
            )
            .unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE catalog_columns_docs (title TEXT, score INT)",
                    vec![],
                )

    .unwrap();
            drop(cassie);

            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            let session = restarted.create_session("tester", None);

            // Act
            let selected = restarted
                .execute_sql(
                    &session,
                    "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = 'catalog_columns_docs' ORDER BY ordinal_position",
                    vec![],
                )

    .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![
                    vec![
                        Value::String("title".to_string()),
                        Value::String("text".to_string())
                    ],
                    vec![
                        Value::String("score".to_string()),
                        Value::String("int".to_string())
                    ]
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reflect_table_rename_drop_lifecycle_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("stable_catalog_lifecycle");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE catalog_lifecycle_before (title TEXT)",
                    vec![],
                )
                .expect("create lifecycle table");
            cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE catalog_lifecycle_before RENAME TO catalog_lifecycle_after",
                    vec![],
                )
                .expect("rename lifecycle table");
            drop(cassie);

            let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
            restarted.startup().expect("restart Cassie");
            let session = restarted.create_session("tester", None);

            // Act
            let renamed = restarted
                .execute_sql(
                    &session,
                    "SELECT table_name FROM information_schema.tables WHERE table_name IN ('catalog_lifecycle_before', 'catalog_lifecycle_after') ORDER BY table_name",
                    vec![],
                )
                .expect("query renamed table metadata");
            let columns = restarted
                .execute_sql(
                    &session,
                    "SELECT table_name, column_name FROM information_schema.columns WHERE table_name = 'catalog_lifecycle_after' ORDER BY ordinal_position",
                    vec![],
                )
                .expect("query renamed column metadata");
            restarted
                .execute_sql(&session, "DROP TABLE catalog_lifecycle_after", vec![])
                .expect("drop lifecycle table");
            let dropped = restarted
                .execute_sql(
                    &session,
                    "SELECT table_name FROM information_schema.tables WHERE table_name = 'catalog_lifecycle_after'",
                    vec![],
                )
                .expect("query dropped table metadata");

            // Assert
            assert_eq!(
                renamed.rows,
                vec![vec![Value::String("catalog_lifecycle_after".to_string())]]
            );
            assert_eq!(
                columns.rows,
                vec![vec![
                    Value::String("catalog_lifecycle_after".to_string()),
                    Value::String("title".to_string()),
                ]]
            );
            assert!(dropped.rows.is_empty());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_list_indexes_through_pg_catalog() {
        // Arrange
        use_local_storage();
        let path = data_dir("indexes");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE catalog_index_docs (email TEXT)",
                    vec![],
                )

    .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE UNIQUE INDEX catalog_email_idx ON catalog_index_docs USING btree (email)",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT indexname FROM pg_catalog.pg_indexes WHERE tablename = 'catalog_index_docs'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("catalog_email_idx".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_list_primary_key_index_through_pg_catalog() {
        // Arrange
        use_local_storage();
        let path = data_dir("primary_key_index");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE catalog_primary_key_docs (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT indexname, indexdef FROM pg_catalog.pg_indexes WHERE tablename = 'catalog_primary_key_docs'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(selected.rows.len(), 1);
            assert_eq!(
                selected.rows[0][0],
                Value::String("catalog_primary_key_docs_pkey".to_string())
            );
            assert_eq!(
                selected.rows[0][1],
                Value::String(
                    "CREATE UNIQUE INDEX catalog_primary_key_docs_pkey ON catalog_primary_key_docs (id)"
                        .to_string()
                )
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_list_composite_indexes_through_pg_catalog() {
        // Arrange
        use_local_storage();
        let path = data_dir("composite_indexes");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE catalog_composite_index_docs (title TEXT, score INT)",
                    vec![],
                )

    .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX catalog_title_score_idx ON catalog_composite_index_docs USING btree (title, score)",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT indexname, indexdef FROM pg_catalog.pg_indexes WHERE tablename = 'catalog_composite_index_docs'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![
                    Value::String("catalog_title_score_idx".to_string()),
                    Value::String(
                        "CREATE INDEX catalog_title_score_idx ON catalog_composite_index_docs (title, score)"
                            .to_string()
                    ),
                ]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_column_store_storage_metadata_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("column_store_storage_restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = CassieRuntimeConfig {
                ..CassieRuntimeConfig::default()
            };

            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE catalog_column_store_docs (doc_id TEXT, title TEXT, score INT) WITH (storage = column_store)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO catalog_column_store_docs (doc_id, title, score) VALUES ('d1', 'alpha', 7)",
                    vec![],
                )
                .unwrap();
            drop(cassie);

            // Act
            let restarted = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            restarted.startup().unwrap();
            let session = restarted.create_session("tester", None);
            let storage = restarted
                .execute_sql(
                    &session,
                    "SELECT tablename, storage_mode, storage_version FROM pg_catalog.pg_table_storage WHERE tablename = 'catalog_column_store_docs'",
                    vec![],
                )
                .unwrap();
            let selected = restarted
                .execute_sql(
                    &session,
                    "SELECT doc_id, title, score FROM catalog_column_store_docs",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                storage.rows,
                vec![vec![
                    Value::String("catalog_column_store_docs".to_string()),
                    Value::String("column-store".to_string()),
                    Value::Int64(1),
                ]]
            );
            assert_eq!(
                selected.rows,
                vec![vec![
                    Value::String("d1".to_string()),
                    Value::String("alpha".to_string()),
                    Value::Int64(7),
                ]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_list_namespaces_through_pg_catalog() {
        // Arrange
        use_local_storage();
        let path = data_dir("namespaces");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(&session, "CREATE SCHEMA analytics", vec![])
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT nspname FROM pg_catalog.pg_namespace ORDER BY nspname",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![
                    vec![Value::String("analytics".to_string())],
                    vec![Value::String("information_schema".to_string())],
                    vec![Value::String("pg_catalog".to_string())],
                    vec![Value::String("public".to_string())],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_list_constraints_through_information_schema() {
        // Arrange
        use_local_storage();
        let path = data_dir("constraints");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE catalog_constraint_docs (email TEXT UNIQUE, score INT CHECK (score >= 0))",
                    vec![],
                )

    .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT constraint_type FROM information_schema.table_constraints WHERE table_name = 'catalog_constraint_docs' ORDER BY constraint_type",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![
                    vec![Value::String("CHECK".to_string())],
                    vec![Value::String("UNIQUE".to_string())]
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_return_admin_role_for_pg_roles_catalog_view() {
        // Arrange
        use_local_storage();
        let path = data_dir("empty_pg_roles");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let selected = cassie
                .execute_sql(&session, "SELECT rolname FROM pg_catalog.pg_roles", vec![])
                .unwrap();

            // Assert
            assert_eq!(selected.rows, vec![vec![Value::String("root".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_list_supported_types_through_pg_catalog_type_view() {
        // Arrange
        use_local_storage();
        let path = data_dir("pg_type");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT typname, oid, typelem, typnamespace FROM pg_catalog.pg_type WHERE typname IN ('smallint', 'bigint', 'bytea', 'char(1)', 'varchar(8)', 'int', 'int[]', 'vector(2)', 'text', 'bytea[]') ORDER BY typname",
                    vec![],
                )

    .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![
                    vec![
                        Value::String("bigint".to_string()),
                        Value::Int64(DataType::BigInt.type_oid()),
                        Value::Int64(0),
                        Value::String("pg_catalog".to_string())
                    ],
                    vec![
                        Value::String("bytea".to_string()),
                        Value::Int64(DataType::Bytea.type_oid()),
                        Value::Int64(0),
                        Value::String("pg_catalog".to_string())
                    ],
                    vec![
                        Value::String("bytea[]".to_string()),
                        Value::Int64(DataType::Array(Box::new(DataType::Bytea)).type_oid()),
                        Value::Int64(DataType::Bytea.type_oid()),
                        Value::String("pg_catalog".to_string())
                    ],
                    vec![
                        Value::String("char(1)".to_string()),
                        Value::Int64(DataType::Char { length: Some(1) }.type_oid()),
                        Value::Int64(0),
                        Value::String("pg_catalog".to_string())
                    ],
                    vec![
                        Value::String("int".to_string()),
                        Value::Int64(DataType::Int.type_oid()),
                        Value::Int64(0),
                        Value::String("pg_catalog".to_string())
                    ],
                    vec![
                        Value::String("int[]".to_string()),
                        Value::Int64(DataType::Array(Box::new(DataType::Int)).type_oid()),
                        Value::Int64(DataType::Int.type_oid()),
                        Value::String("pg_catalog".to_string())
                    ],
                    vec![
                        Value::String("smallint".to_string()),
                        Value::Int64(DataType::SmallInt.type_oid()),
                        Value::Int64(0),
                        Value::String("pg_catalog".to_string())
                    ],
                    vec![
                        Value::String("text".to_string()),
                        Value::Int64(DataType::Text.type_oid()),
                        Value::Int64(0),
                        Value::String("pg_catalog".to_string())
                    ],
                    vec![
                        Value::String("varchar(8)".to_string()),
                        Value::Int64(DataType::Varchar { length: Some(8) }.type_oid()),
                        Value::Int64(0),
                        Value::String("pg_catalog".to_string())
                    ],
                    vec![
                        Value::String("vector(2)".to_string()),
                        Value::Int64(DataType::Vector(2).type_oid()),
                        Value::Int64(0),
                        Value::String("pg_catalog".to_string())
                    ],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_list_user_defined_views_through_catalog_views() {
        // Arrange
        use_local_storage();
        let path = data_dir("views");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE catalog_views_docs (title TEXT, score INT)",
                    vec![],
                )

    .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE VIEW catalog_views_ready AS SELECT title, score FROM catalog_views_docs",
                    vec![],
                )
                .unwrap();

            // Act
            let tables = cassie
                .execute_sql(
                    &session,
                    "SELECT table_type FROM information_schema.tables WHERE table_name = 'catalog_views_ready'",
                    vec![],
                )
                .unwrap();
            let views = cassie
                .execute_sql(
                    &session,
                    "SELECT table_name FROM information_schema.views WHERE table_name = 'catalog_views_ready'",
                    vec![],
                )
                .unwrap();
            let classes = cassie
                .execute_sql(
                    &session,
                    "SELECT relkind FROM pg_catalog.pg_class WHERE relname = 'catalog_views_ready'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(tables.rows, vec![vec![Value::String("VIEW".to_string())]]);
            assert_eq!(views.rows, vec![vec![Value::String("catalog_views_ready".to_string())]]);
            assert_eq!(classes.rows, vec![vec![Value::String("v".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }
}
// Formerly tests/catalog_introspection_foreign_keys.rs.
mod catalog_introspection_foreign_keys {
    use cassie::app::{Cassie, CassieSession};
    use cassie::types::Value;

    use super::support_catalog as support;
    use super::support_data_dir as data_dir;
    use super::support_local_storage as local_storage;
    use data_dir::data_dir;
    use local_storage::use_local_storage;
    use support::{execute_statement, query_rows};

    struct NamedForeignKeyRows {
        table_constraints: Vec<Vec<Value>>,
        key_usage: Vec<Vec<Value>>,
        referential: Vec<Vec<Value>>,
        pg_constraint: Vec<Vec<Value>>,
    }

    fn apply_named_foreign_key_schema(cassie: &Cassie, session: &CassieSession) {
        for sql in [
            r#"CREATE TABLE "catalog_named_fk_parents" (
            "id" INT,
            CONSTRAINT "catalog_named_fk_parents_pkey" PRIMARY KEY ("id")
        )"#,
            r#"CREATE TABLE "catalog_named_fk_children" (
            "id" INT,
            "parent_id" INT,
            CONSTRAINT "catalog_named_fk_children_pkey" PRIMARY KEY ("id")
        )"#,
            r#"ALTER TABLE "catalog_named_fk_children"
            ADD CONSTRAINT "catalog_named_fk_children_parent_fkey"
            FOREIGN KEY ("parent_id") REFERENCES "catalog_named_fk_parents"("id")
            ON DELETE CASCADE ON UPDATE CASCADE"#,
        ] {
            execute_statement(cassie, session, sql);
        }
    }

    fn collect_named_foreign_key_rows(
        cassie: &Cassie,
        session: &CassieSession,
    ) -> NamedForeignKeyRows {
        NamedForeignKeyRows {
        table_constraints: query_rows(
            cassie,
            session,
            "SELECT constraint_name, constraint_type FROM information_schema.table_constraints WHERE table_name = 'catalog_named_fk_children' ORDER BY constraint_name",
        ),
        key_usage: query_rows(
            cassie,
            session,
            "SELECT constraint_name, column_name, ordinal_position FROM information_schema.key_column_usage WHERE table_name = 'catalog_named_fk_children' ORDER BY constraint_name",
        ),
        referential: query_rows(
            cassie,
            session,
            "SELECT constraint_name, unique_constraint_name, update_rule, delete_rule FROM information_schema.referential_constraints WHERE constraint_name = 'catalog_named_fk_children_parent_fkey'",
        ),
        pg_constraint: query_rows(
            cassie,
            session,
            "SELECT conname, contype FROM pg_catalog.pg_constraint WHERE conrelid = 'catalog_named_fk_children' ORDER BY conname",
        ),
    }
    }

    fn assert_named_foreign_key_rows(rows: &NamedForeignKeyRows) {
        assert_eq!(
            rows.table_constraints,
            vec![
                vec![
                    Value::String("catalog_named_fk_children_parent_fkey".to_string()),
                    Value::String("FOREIGN KEY".to_string())
                ],
                vec![
                    Value::String("catalog_named_fk_children_pkey".to_string()),
                    Value::String("PRIMARY KEY".to_string())
                ],
            ]
        );
        assert_eq!(
            rows.key_usage,
            vec![
                vec![
                    Value::String("catalog_named_fk_children_parent_fkey".to_string()),
                    Value::String("parent_id".to_string()),
                    Value::Int64(1)
                ],
                vec![
                    Value::String("catalog_named_fk_children_pkey".to_string()),
                    Value::String("id".to_string()),
                    Value::Int64(1)
                ],
            ]
        );
        assert_eq!(
            rows.referential,
            vec![vec![
                Value::String("catalog_named_fk_children_parent_fkey".to_string()),
                Value::String("catalog_named_fk_parents_pkey".to_string()),
                Value::String("CASCADE".to_string()),
                Value::String("CASCADE".to_string())
            ]]
        );
        assert_eq!(
            rows.pg_constraint,
            vec![
                vec![
                    Value::String("catalog_named_fk_children_id_n".to_string()),
                    Value::String("n".to_string())
                ],
                vec![
                    Value::String("catalog_named_fk_children_parent_fkey".to_string()),
                    Value::String("f".to_string())
                ],
                vec![
                    Value::String("catalog_named_fk_children_pkey".to_string()),
                    Value::String("p".to_string())
                ],
            ]
        );
    }

    #[test]
    fn should_list_named_foreign_key_metadata_through_catalog_views() {
        // Arrange
        use_local_storage();
        let path = data_dir("named_fk_metadata");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            apply_named_foreign_key_schema(&cassie, &session);

            // Act
            let rows = collect_named_foreign_key_rows(&cassie, &session);

            // Assert
            assert_named_foreign_key_rows(&rows);

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/catalog_introspection_pgadmin.rs.
mod catalog_introspection_pgadmin {
    use cassie::app::{Cassie, CassieSession};
    use cassie::types::Value;

    use super::support_catalog as support;
    use super::support_data_dir as data_dir;
    use super::support_local_storage as local_storage;
    use data_dir::data_dir;
    use local_storage::use_local_storage;
    use support::{execute_statement, query_rows};

    struct PgAdminCatalogRows {
        namespace: Vec<Vec<Value>>,
        classes: Vec<Vec<Value>>,
        columns: Vec<Vec<Value>>,
        defaults: Vec<Vec<Value>>,
        indexes: Vec<Vec<Value>>,
        constraints: Vec<Vec<Value>>,
        view: Vec<Vec<Value>>,
        table_data: Vec<Vec<Value>>,
    }

    fn apply_pgadmin_catalog_fixture(cassie: &Cassie, session: &CassieSession) {
        for sql in [
            r#"CREATE TABLE "catalog_pgadmin_docs" (
            "id" INT DEFAULT 1,
            "title" VARCHAR(32) NOT NULL,
            "score" INT,
            CONSTRAINT "catalog_pgadmin_docs_pkey" PRIMARY KEY ("id"),
            CONSTRAINT "catalog_pgadmin_docs_score_check" CHECK (score >= 0)
        )"#,
            r#"CREATE INDEX "catalog_pgadmin_docs_title_idx"
            ON "catalog_pgadmin_docs" ("title")"#,
            "CREATE VIEW catalog_pgadmin_ready AS SELECT title, score FROM catalog_pgadmin_docs",
            "INSERT INTO catalog_pgadmin_docs (id, title, score) VALUES (1, 'alpha', 9)",
        ] {
            execute_statement(cassie, session, sql);
        }
    }

    fn collect_pgadmin_catalog_rows(
        cassie: &Cassie,
        session: &CassieSession,
    ) -> PgAdminCatalogRows {
        PgAdminCatalogRows {
        namespace: query_rows(
            cassie,
            session,
            "SELECT oid, nspname, pg_catalog.pg_get_userbyid(nspowner), pg_catalog.has_schema_privilege(nspname, 'USAGE') FROM pg_catalog.pg_namespace WHERE nspname = 'public'",
        ),
        classes: query_rows(
            cassie,
            session,
            "SELECT relname, relkind, relnamespace_oid, relhasindex, relpersistence, pg_catalog.quote_ident(relname), pg_catalog.has_table_privilege(relname, 'SELECT'), pg_catalog.pg_table_is_visible(oid) FROM pg_catalog.pg_class WHERE relname IN ('catalog_pgadmin_docs', 'catalog_pgadmin_docs_title_idx', 'catalog_pgadmin_ready') ORDER BY relname",
        ),
        columns: query_rows(
            cassie,
            session,
            "SELECT attname, attnum, pg_catalog.format_type(atttypid, atttypmod), attnotnull, atthasdef, attrelid_oid, attisdropped FROM pg_catalog.pg_attribute WHERE attrelid = 'catalog_pgadmin_docs' ORDER BY attnum",
        ),
        defaults: query_rows(
            cassie,
            session,
            "SELECT adsrc FROM pg_catalog.pg_attrdef WHERE adrelid = 'catalog_pgadmin_docs' ORDER BY adnum",
        ),
        indexes: query_rows(
            cassie,
            session,
            "SELECT indexrelid, indrelid, indexrelid_oid, indrelid_oid, indisunique, indisprimary, indisvalid FROM pg_catalog.pg_index WHERE indrelid = 'catalog_pgadmin_docs' ORDER BY indexrelid",
        ),
        constraints: query_rows(
            cassie,
            session,
            "SELECT conname, conrelid, conrelid_oid, contype, conkey, convalidated FROM pg_catalog.pg_constraint WHERE conrelid = 'catalog_pgadmin_docs' ORDER BY conname",
        ),
        view: query_rows(
            cassie,
            session,
            "SELECT table_name, view_definition FROM information_schema.views WHERE table_name = 'catalog_pgadmin_ready'",
        ),
        table_data: query_rows(
            cassie,
            session,
            "SELECT title, score FROM catalog_pgadmin_docs ORDER BY title",
        ),
    }
    }

    fn assert_pgadmin_namespace(rows: &PgAdminCatalogRows) -> i64 {
        assert_eq!(rows.namespace.len(), 1);
        assert_eq!(rows.namespace[0][1], Value::String("public".to_string()));
        assert_eq!(rows.namespace[0][2], Value::String("root".to_string()));
        assert_eq!(rows.namespace[0][3], Value::Bool(true));
        let Value::Int64(public_oid) = rows.namespace[0][0] else {
            panic!("public namespace oid should be numeric");
        };
        public_oid
    }

    fn assert_pgadmin_classes(rows: &PgAdminCatalogRows, public_oid: i64) {
        assert_eq!(
            rows.classes,
            vec![
                pgadmin_class_row("catalog_pgadmin_docs", "r", true, public_oid),
                pgadmin_class_row("catalog_pgadmin_docs_title_idx", "i", false, public_oid),
                pgadmin_class_row("catalog_pgadmin_ready", "v", false, public_oid),
            ]
        );
    }

    fn pgadmin_class_row(
        name: &str,
        relkind: &str,
        has_index: bool,
        public_oid: i64,
    ) -> Vec<Value> {
        vec![
            Value::String(name.to_string()),
            Value::String(relkind.to_string()),
            Value::Int64(public_oid),
            Value::Bool(has_index),
            Value::String("p".to_string()),
            Value::String(name.to_string()),
            Value::Bool(true),
            Value::Bool(true),
        ]
    }

    fn assert_pgadmin_columns(rows: &PgAdminCatalogRows) -> i64 {
        let Value::Int64(table_oid) = rows.columns[0][5] else {
            panic!("table oid should be numeric");
        };
        assert!(table_oid > 0);
        assert_eq!(
            rows.columns,
            vec![
                pgadmin_column_row("id", 1, "integer", true, true, table_oid),
                pgadmin_column_row("title", 2, "character varying(32)", true, false, table_oid),
                pgadmin_column_row("score", 3, "integer", false, false, table_oid),
            ]
        );
        table_oid
    }

    fn pgadmin_column_row(
        name: &str,
        attnum: i64,
        data_type: &str,
        not_null: bool,
        has_default: bool,
        table_oid: i64,
    ) -> Vec<Value> {
        vec![
            Value::String(name.to_string()),
            Value::Int64(attnum),
            Value::String(data_type.to_string()),
            Value::Bool(not_null),
            Value::Bool(has_default),
            Value::Int64(table_oid),
            Value::Bool(false),
        ]
    }

    fn assert_pgadmin_indexes(rows: &PgAdminCatalogRows, table_oid: i64) {
        assert_eq!(rows.defaults, vec![vec![Value::String("1".to_string())]]);
        assert_eq!(rows.indexes.len(), 2);
        assert_eq!(
            rows.indexes
                .iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>(),
            vec![
                Value::String("catalog_pgadmin_docs_pkey".to_string()),
                Value::String("catalog_pgadmin_docs_title_idx".to_string()),
            ]
        );
        assert!(rows.indexes.iter().all(|row| row[2].as_i64().is_some()));
        assert!(rows
            .indexes
            .iter()
            .all(|row| row[3] == Value::Int64(table_oid)));
        assert!(rows.indexes.iter().all(|row| row[6] == Value::Bool(true)));
    }

    fn assert_pgadmin_constraints(rows: &PgAdminCatalogRows, table_oid: i64) {
        assert_eq!(
            rows.constraints,
            vec![
                pgadmin_constraint_row("catalog_pgadmin_docs_id_n", "n", "1", table_oid),
                pgadmin_constraint_row("catalog_pgadmin_docs_pkey", "p", "1", table_oid),
                pgadmin_constraint_row("catalog_pgadmin_docs_score_check", "c", "3", table_oid),
                pgadmin_constraint_row("catalog_pgadmin_docs_title_n", "n", "2", table_oid),
            ]
        );
    }

    fn pgadmin_constraint_row(name: &str, kind: &str, key: &str, table_oid: i64) -> Vec<Value> {
        vec![
            Value::String(name.to_string()),
            Value::String("catalog_pgadmin_docs".to_string()),
            Value::Int64(table_oid),
            Value::String(kind.to_string()),
            Value::String(key.to_string()),
            Value::Bool(true),
        ]
    }

    fn assert_pgadmin_view_and_data(rows: &PgAdminCatalogRows) {
        assert_eq!(
            rows.view,
            vec![vec![
                Value::String("catalog_pgadmin_ready".to_string()),
                Value::String("SELECT title, score FROM catalog_pgadmin_docs".to_string()),
            ]]
        );
        assert_eq!(
            rows.table_data,
            vec![vec![Value::String("alpha".to_string()), Value::Int64(9)]]
        );
    }

    fn assert_pgadmin_catalog_rows(rows: &PgAdminCatalogRows) {
        let public_oid = assert_pgadmin_namespace(rows);
        assert_pgadmin_classes(rows, public_oid);
        let table_oid = assert_pgadmin_columns(rows);
        assert_pgadmin_indexes(rows, table_oid);
        assert_pgadmin_constraints(rows, table_oid);
        assert_pgadmin_view_and_data(rows);
    }

    #[test]
    fn should_support_pgadmin_browser_workflow_catalog_queries() {
        // Arrange
        use_local_storage();
        let path = data_dir("pgadmin_browser");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", Some("postgres".to_string()));
            apply_pgadmin_catalog_fixture(&cassie, &session);

            // Act
            let rows = collect_pgadmin_catalog_rows(&cassie, &session);

            // Assert
            assert_pgadmin_catalog_rows(&rows);

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/catalog_orm_metadata.rs.
mod catalog_orm_metadata {
    use cassie::app::Cassie;
    use cassie::types::Value;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn use_local_storage() {
        if std::env::var("CASSIE_EMBEDDINGS_PROVIDER").is_err() {
            std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "fallback");
        }
    }

    fn data_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cassie-catalog-orm-{name}-{}", Uuid::new_v4()))
    }

    struct OrmMetadataRows {
        columns: Vec<Vec<Value>>,
        attributes: Vec<Vec<Value>>,
        defaults: Vec<Vec<Value>>,
        indexes: Vec<Vec<Value>>,
    }

    fn assert_orm_columns(rows: &OrmMetadataRows) {
        assert_eq!(
            rows.columns,
            vec![
                vec![
                    Value::String("id".to_string()),
                    Value::Int64(1),
                    Value::String("NO".to_string()),
                    Value::String("int".to_string()),
                    Value::String("int4".to_string()),
                    Value::String("7".to_string()),
                    Value::Null,
                    Value::Int64(32),
                    Value::Int64(0),
                    Value::Null,
                ],
                vec![
                    Value::String("code".to_string()),
                    Value::Int64(2),
                    Value::String("NO".to_string()),
                    Value::String("varchar(16)".to_string()),
                    Value::String("varchar".to_string()),
                    Value::Null,
                    Value::Int64(16),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                ],
                vec![
                    Value::String("label".to_string()),
                    Value::Int64(3),
                    Value::String("YES".to_string()),
                    Value::String("text".to_string()),
                    Value::String("text".to_string()),
                    Value::String("'new'".to_string()),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Null,
                ],
            ]
        );
    }

    fn assert_orm_attributes(rows: &OrmMetadataRows) {
        assert_eq!(
            rows.attributes,
            vec![
                vec![
                    Value::String("id".to_string()),
                    Value::Int64(1),
                    Value::Int64(23),
                    Value::Bool(true),
                    Value::Int64(-1),
                    Value::Bool(true),
                ],
                vec![
                    Value::String("code".to_string()),
                    Value::Int64(2),
                    Value::Int64(1043),
                    Value::Bool(true),
                    Value::Int64(20),
                    Value::Bool(false),
                ],
                vec![
                    Value::String("label".to_string()),
                    Value::Int64(3),
                    Value::Int64(25),
                    Value::Bool(false),
                    Value::Int64(-1),
                    Value::Bool(true),
                ],
            ]
        );
    }

    fn assert_orm_defaults(rows: &OrmMetadataRows) {
        assert_eq!(
            rows.defaults,
            vec![
                vec![
                    Value::String("catalog_orm_parents".to_string()),
                    Value::Int64(1),
                    Value::String("7".to_string()),
                ],
                vec![
                    Value::String("catalog_orm_parents".to_string()),
                    Value::Int64(3),
                    Value::String("'new'".to_string()),
                ],
            ]
        );
    }

    fn assert_orm_indexes(rows: &OrmMetadataRows) {
        assert_eq!(
            rows.indexes,
            vec![
                vec![
                    Value::String("catalog_orm_children_parent_idx".to_string()),
                    Value::String("catalog_orm_children".to_string()),
                    Value::Bool(false),
                    Value::Bool(false),
                    Value::String("2".to_string()),
                ],
                vec![
                    Value::String("catalog_orm_children_pkey".to_string()),
                    Value::String("catalog_orm_children".to_string()),
                    Value::Bool(true),
                    Value::Bool(true),
                    Value::String("1".to_string()),
                ],
            ]
        );
    }

    fn apply_orm_metadata_fixture(cassie: &Cassie) {
        let session = cassie.create_session("tester", None);
        for sql in [
            r#"CREATE TABLE "catalog_orm_parents" (
            "id" INT NOT NULL DEFAULT 7,
            "code" VARCHAR(16) NOT NULL,
            "label" TEXT DEFAULT 'new',
            CONSTRAINT "catalog_orm_parents_pkey" PRIMARY KEY ("id"),
            CONSTRAINT "catalog_orm_parents_code_key" UNIQUE ("code"),
            CONSTRAINT "catalog_orm_parents_id_check" CHECK (id > 0)
        )"#,
            r#"CREATE TABLE "catalog_orm_children" (
            "id" INT NOT NULL,
            "parent_id" INT DEFAULT 7,
            CONSTRAINT "catalog_orm_children_pkey" PRIMARY KEY ("id"),
            CONSTRAINT "catalog_orm_children_parent_fkey"
                FOREIGN KEY ("parent_id") REFERENCES "catalog_orm_parents"("id")
                ON DELETE SET NULL ON UPDATE CASCADE
        )"#,
            r#"CREATE INDEX "catalog_orm_children_parent_idx"
            ON "catalog_orm_children" ("parent_id")"#,
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
    }

    fn query_rows(cassie: &Cassie, sql: &str) -> Vec<Vec<Value>> {
        let session = cassie.create_session("tester", None);
        cassie.execute_sql(&session, sql, vec![]).unwrap().rows
    }

    fn collect_orm_metadata_rows(cassie: &Cassie) -> OrmMetadataRows {
        OrmMetadataRows {
        columns: query_rows(
            cassie,
            "SELECT column_name, ordinal_position, is_nullable, data_type, udt_name, column_default, character_maximum_length, numeric_precision, numeric_scale, datetime_precision FROM information_schema.columns WHERE table_name = 'catalog_orm_parents' ORDER BY ordinal_position",
        ),
        attributes: query_rows(
            cassie,
            "SELECT attname, attnum, atttypid, attnotnull, atttypmod, atthasdef FROM pg_catalog.pg_attribute WHERE attrelid = 'catalog_orm_parents' ORDER BY attnum",
        ),
        defaults: query_rows(
            cassie,
            "SELECT adrelid, adnum, adsrc FROM pg_catalog.pg_attrdef WHERE adrelid = 'catalog_orm_parents' ORDER BY adnum",
        ),
        indexes: query_rows(
            cassie,
            "SELECT indexrelid, indrelid, indisunique, indisprimary, indkey FROM pg_catalog.pg_index WHERE indrelid = 'catalog_orm_children' ORDER BY indexrelid",
        ),
    }
    }

    fn assert_orm_metadata_rows(rows: &OrmMetadataRows) {
        assert_orm_columns(rows);
        assert_orm_attributes(rows);
        assert_orm_defaults(rows);
        assert_orm_indexes(rows);
    }

    #[test]
    fn should_expose_orm_introspection_metadata_through_catalog_views() {
        // Arrange
        use_local_storage();
        let path = data_dir("metadata");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            apply_orm_metadata_fixture(&cassie);

            // Act
            let rows = collect_orm_metadata_rows(&cassie);

            // Assert
            assert_orm_metadata_rows(&rows);

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/catalog_probes.rs.
mod catalog_probes {
    use super::support_sql as support;
    use cassie::app::Cassie;
    use cassie::types::{DataType, Value};
    use support::*;

    #[test]
    fn should_return_version_function() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("version");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", Some("catalogdb".to_string()));

            // Act
            let result = cassie
                .execute_sql(&session, "SELECT version()", vec![])
                .unwrap();

            // Assert
            assert_eq!(result.columns[0].name, "version");
            assert_eq!(result.rows.len(), 1);
            assert!(matches!(
                &result.rows[0][0],
                Value::String(version) if version.starts_with("PostgreSQL 16.0 compatible Cassie ")
            ));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_return_postgres_shaped_pg_catalog_version_function() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("pg_catalog_version");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", Some("catalogdb".to_string()));

            // Act
            let result = cassie
                .execute_sql(&session, "SELECT pg_catalog.version()", vec![])
                .unwrap();

            // Assert
            assert_eq!(result.columns[0].name, "pg_catalog.version");
            assert_eq!(result.rows.len(), 1);
            let Value::String(version) = &result.rows[0][0] else {
                panic!("pg_catalog.version should return text");
            };
            assert!(version.starts_with("PostgreSQL 16.0"));
            assert!(version.contains(env!("CARGO_PKG_VERSION")));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_return_current_schema_function() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("schema");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", Some("catalogdb".to_string()));

            // Act
            let result = cassie
                .execute_sql(&session, "SELECT current_schema()", vec![])
                .unwrap();

            // Assert
            assert_eq!(result.columns[0].name, "current_schema");
            assert_eq!(result.rows.len(), 1);
            assert_eq!(result.rows[0][0], Value::String("public".to_string()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_return_current_database_function() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("database");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", Some("catalogdb".to_string()));

            // Act
            let result = cassie
                .execute_sql(&session, "SELECT current_database()", vec![])
                .unwrap();

            // Assert
            assert_eq!(result.columns[0].name, "current_database");
            assert_eq!(result.rows.len(), 1);
            assert_eq!(result.rows[0][0], Value::String("catalogdb".to_string()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_return_search_path_from_show_statement() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("show_search_path");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", Some("catalogdb".to_string()));

            // Act
            let result = cassie.execute_sql(&session, "SHOW search_path", vec![]);

            // Assert
            let result = result.unwrap();
            assert_eq!(
                result.columns,
                vec![cassie::executor::ColumnMeta {
                    name: "search_path".to_string(),
                    data_type: "text".to_string(),
                    type_oid: DataType::Text.type_oid(),
                    typlen: DataType::Text.typlen(),
                    atttypmod: DataType::Text.atttypmod(),
                    format_code: 0,
                    nullable: true,
                }]
            );
            assert_eq!(result.rows, vec![vec![Value::String("public".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_return_sqlalchemy_dialect_show_metadata() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("show_sqlalchemy_metadata");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", Some("catalogdb".to_string()));

            // Act
            let isolation = cassie
                .execute_sql(&session, "SHOW transaction isolation level", vec![])
                .unwrap();
            let strings = cassie
                .execute_sql(&session, "SHOW standard_conforming_strings", vec![])
                .unwrap();

            // Assert
            assert_eq!(isolation.columns[0].name, "transaction_isolation");
            assert_eq!(
                isolation.rows,
                vec![vec![Value::String("read committed".to_string())]]
            );
            assert_eq!(strings.columns[0].name, "standard_conforming_strings");
            assert_eq!(strings.rows, vec![vec![Value::String("on".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_treat_supported_set_statement_as_noop() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("set_supported");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", Some("catalogdb".to_string()));

            // Act
            let result = cassie.execute_sql(&session, "SET search_path = public", vec![]);

            // Assert
            let result = result.unwrap();
            assert_eq!(result.command, "SET");
            assert!(result.columns.is_empty());
            assert!(result.rows.is_empty());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_unsupported_show_variable() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("show_unsupported");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", Some("catalogdb".to_string()));

            // Act
            let result = cassie.execute_sql(&session, "SHOW unsupported_metadata", vec![]);

            // Assert
            assert!(result.is_err());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_unsupported_set_variable() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("set_unsupported");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", Some("catalogdb".to_string()));

            // Act
            let result = cassie.execute_sql(&session, "SET unsupported_variable = foo", vec![]);

            // Assert
            assert!(result.is_err());

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/database_connect_grants.rs.
mod database_connect_grants {
    use cassie::app::{Cassie, CassieError, CatalogObjectKind};
    use cassie::sql::ast::QueryStatement;

    fn cassie(label: &str) -> Cassie {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = std::env::temp_dir().join(format!(
            "cassie-database-connect-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        let cassie = Cassie::new_with_data_dir(path).expect("cassie");
        cassie.startup().expect("startup");
        cassie
    }

    fn setup_reader(cassie: &Cassie) -> cassie::app::CassieSession {
        let admin = cassie
            .authenticate_role("root", Some("postgres"), None)
            .expect("admin");
        cassie
            .execute_sql(&admin, "CREATE DATABASE analytics", Vec::new())
            .expect("database");
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE reader LOGIN PASSWORD 'reader-secret'",
                Vec::new(),
            )
            .expect("reader");
        admin
    }

    #[test]
    fn should_parse_exact_database_connect_grant_forms() {
        // Arrange
        let grant_sql = "GRANT CONNECT ON DATABASE analytics TO reader";
        let revoke_sql = "REVOKE CONNECT ON DATABASE analytics FROM reader";

        // Act
        let grant = cassie::sql::parse_statement(grant_sql).expect("grant");
        let revoke = cassie::sql::parse_statement(revoke_sql).expect("revoke");

        // Assert
        assert!(matches!(
            grant.statement,
            QueryStatement::GrantDatabaseConnect(statement)
                if statement.database == "analytics" && statement.role == "reader"
        ));
        assert!(matches!(
            revoke.statement,
            QueryStatement::RevokeDatabaseConnect(statement)
                if statement.database == "analytics" && statement.role == "reader"
        ));
    }

    #[test]
    fn should_revoke_live_database_access_given_a_connect_grant_removal() {
        // Arrange
        let cassie = cassie("live-revalidation");
        let admin = setup_reader(&cassie);
        for _ in 0..2 {
            cassie
                .execute_sql(
                    &admin,
                    "GRANT CONNECT ON DATABASE analytics TO reader",
                    Vec::new(),
                )
                .expect("grant");
        }
        let reader = cassie
            .authenticate_role(
                "reader",
                Some("reader-secret"),
                Some("analytics".to_string()),
            )
            .expect("reader session");
        cassie
            .execute_sql(&reader, "SELECT 1", Vec::new())
            .expect("granted query");

        // Act
        for _ in 0..2 {
            cassie
                .execute_sql(
                    &admin,
                    "REVOKE CONNECT ON DATABASE analytics FROM reader",
                    Vec::new(),
                )
                .expect("revoke");
        }
        let revoked = cassie
            .execute_sql(&reader, "SELECT 1", Vec::new())
            .expect_err("active session must lose access");
        cassie
            .execute_sql(
                &admin,
                "GRANT CONNECT ON DATABASE analytics TO reader",
                Vec::new(),
            )
            .expect("restore grant");
        let restored = cassie.execute_sql(&reader, "SELECT 1", Vec::new());

        // Assert
        assert!(matches!(revoked, CassieError::InsufficientPrivilege));
        assert!(restored.is_ok());
    }

    #[test]
    fn should_validate_database_connect_grant_authority_targets() {
        // Arrange
        let cassie = cassie("errors");
        let admin = setup_reader(&cassie);
        let reader = cassie
            .authenticate_role("reader", Some("reader-secret"), None)
            .expect("reader");

        // Act
        let denied = cassie
            .execute_sql(
                &reader,
                "GRANT CONNECT ON DATABASE analytics TO reader",
                Vec::new(),
            )
            .expect_err("non-admin grant");
        let missing_database = cassie
            .execute_sql(
                &admin,
                "GRANT CONNECT ON DATABASE missing TO reader",
                Vec::new(),
            )
            .expect_err("missing database");
        let missing_role = cassie
            .execute_sql(
                &admin,
                "GRANT CONNECT ON DATABASE analytics TO missing",
                Vec::new(),
            )
            .expect_err("missing role");
        let implicit_admin = cassie
            .execute_sql(
                &admin,
                "REVOKE CONNECT ON DATABASE analytics FROM root",
                Vec::new(),
            )
            .expect_err("admin access is implicit");

        // Assert
        assert!(matches!(denied, CassieError::InsufficientPrivilege));
        assert!(matches!(
            missing_database,
            CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Database,
                ..
            }
        ));
        assert!(matches!(
            missing_role,
            CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Role,
                ..
            }
        ));
        assert!(matches!(implicit_admin, CassieError::Unsupported(_)));
    }
}

// Formerly tests/database_connect_network.rs.
mod database_connect_network {
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::{Cassie, CassieError, CassieSession};
    use cassie::config::CassieRuntimeConfig;
    use reqwest::{Client, StatusCode};
    use tokio::sync::Notify;
    use tokio_postgres::NoTls;
    use uuid::Uuid;

    fn fixture(label: &str) -> (Cassie, CassieSession, String) {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = std::env::temp_dir()
            .join(format!(
                "cassie-database-connect-network-{label}-{}",
                Uuid::new_v4()
            ))
            .to_string_lossy()
            .to_string();
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let admin = cassie
            .authenticate_role("root", Some("postgres"), None)
            .expect("admin");
        for sql in [
            "CREATE DATABASE analytics",
            "CREATE ROLE reader LOGIN PASSWORD 'reader-secret'",
            "GRANT CONNECT ON DATABASE analytics TO reader",
        ] {
            cassie
                .execute_sql(&admin, sql, Vec::new())
                .expect("database access fixture");
        }
        (cassie, admin, path)
    }

    async fn spawn_pgwire_server(
        cassie: Cassie,
    ) -> (
        std::net::SocketAddr,
        tokio::task::JoinHandle<Result<(), CassieError>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire");
        let address = listener.local_addr().expect("pgwire address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            address.to_string(),
            Arc::new(cassie),
            CassieRuntimeConfig::default(),
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        (address, server)
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
            .expect("bind REST");
        let address = listener.local_addr().expect("REST address");
        drop(listener);
        let shutdown = Arc::new(Notify::new());
        let server = tokio::spawn(cassie::rest::router::run_with_shutdown(
            address.to_string(),
            cassie,
            Arc::clone(&shutdown),
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        (format!("http://{address}"), shutdown, server)
    }

    fn set_database_access(cassie: &Cassie, admin: &CassieSession, grant: bool) {
        let verb = if grant { "GRANT" } else { "REVOKE" };
        let preposition = if grant { "TO" } else { "FROM" };
        cassie
            .execute_sql(
                admin,
                &format!("{verb} CONNECT ON DATABASE analytics {preposition} reader"),
                Vec::new(),
            )
            .expect("change database access");
    }

    #[test]
    fn should_revalidate_connect_privileges_given_an_already_authenticated_session() {
        // Arrange
        let (cassie, admin, path) = fixture("pgwire");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (address, server) = spawn_pgwire_server(cassie.clone()).await;
            let mut config = tokio_postgres::Config::new();
            config
                .host("127.0.0.1")
                .port(address.port())
                .user("reader")
                .password("reader-secret")
                .dbname("analytics");
            let (client, connection) = config.connect(NoTls).await.expect("reader connection");
            let connection = tokio::spawn(connection);
            client
                .simple_query("SELECT 1")
                .await
                .expect("granted query");

            // Act
            set_database_access(&cassie, &admin, false);
            let revoked = client
                .simple_query("SELECT 1")
                .await
                .expect_err("revoked query");
            set_database_access(&cassie, &admin, true);
            let restored = client.simple_query("SELECT 1").await;

            // Assert
            assert_eq!(
                revoked.as_db_error().map(|error| error.code().code()),
                Some("42501")
            );
            assert!(restored.is_ok());
            drop(client);
            connection.abort();
            server.abort();
            let _ = connection.await;
            let _ = server.await;
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_revalidate_database_connect_for_an_active_rest_session() {
        // Arrange
        let (cassie, admin, path) = fixture("rest");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let (base_url, shutdown, server) = spawn_rest_server(cassie.clone()).await;
            let client = Client::new();
            let login = client
                .post(format!("{base_url}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "reader",
                    "password": "reader-secret"
                }))
                .send()
                .await
                .expect("reader login");
            let cookie = login
                .headers()
                .get("set-cookie")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .expect("session cookie")
                .to_string();
            let query = || {
                client
                    .post(format!("{base_url}/api/v1/admin/query-executions"))
                    .header("cookie", &cookie)
                    .json(&serde_json::json!({
                        "database": "analytics",
                        "sql": "SELECT 1"
                    }))
                    .send()
            };
            let granted = query().await.expect("granted query");

            // Act
            set_database_access(&cassie, &admin, false);
            let revoked = query().await.expect("revoked query");
            set_database_access(&cassie, &admin, true);
            let restored = query().await.expect("restored query");

            // Assert
            assert_eq!(granted.status(), StatusCode::OK);
            assert_eq!(revoked.status(), StatusCode::FORBIDDEN);
            assert_eq!(restored.status(), StatusCode::OK);
            shutdown.notify_waiters();
            let _ = server.await;
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/database_images.rs.
mod database_images {
    use super::support_executor as support;
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::types::{DataType, FieldSchema, Schema};
    use support::*;

    #[test]
    fn should_round_trip_one_database_as_bounded_chunks() {
        // Arrange
        let source_path = data_dir("source");
        let cassie = Cassie::new_with_data_dir(&source_path).expect("cassie");
        cassie.startup().expect("startup");
        cassie
            .midge
            .create_database("analytics", None)
            .expect("database");
        cassie
            .midge
            .create_namespace("analytics.public")
            .expect("namespace");
        let source_collection = canonical_relation_name("analytics", "public", "docs");
        cassie
            .midge
            .create_collection(
                &source_collection,
                Schema {
                    fields: vec![FieldSchema {
                        name: "value".to_string(),
                        data_type: DataType::Text,
                        nullable: false,
                    }],
                },
            )
            .expect("collection");
        cassie
            .midge
            .put_document(
                &source_collection,
                Some("row-1".to_string()),
                serde_json::json!({"value": "from-image"}),
            )
            .expect("row");

        let mut backup = cassie
            .begin_database_backup("analytics")
            .expect("begin backup");
        let mut image = Vec::new();
        while let Some(chunk) = backup.next_chunk().expect("backup chunk") {
            assert!(chunk.len() <= 64 * 1024);
            image.extend_from_slice(&chunk);
        }

        // Act
        let mut restore = cassie
            .begin_database_restore("restored")
            .expect("begin restore");
        for chunk in image.chunks(3) {
            restore.push_chunk(chunk).expect("restore chunk");
        }
        restore.finish().expect("finish restore");
        cassie.hydrate_catalog().expect("hydrate restored catalog");

        // Assert
        let target_collection = canonical_relation_name("restored", "public", "docs");
        let row = cassie
            .midge
            .get_document(&target_collection, "row-1")
            .expect("restored row lookup")
            .expect("restored row");
        assert_eq!(row.payload["value"], "from-image");
        assert!(cassie
            .midge
            .get_document(&source_collection, "row-1")
            .expect("source row lookup")
            .is_some());

        let _ = std::fs::remove_dir_all(source_path);
    }
}

// Formerly tests/database_scope.rs.
mod database_scope {
    use super::support_sql as support;
    use cassie::app::{Cassie, CassieError, CatalogObjectKind};
    use cassie::types::Value;
    use support::*;

    fn seed_catalog_filtering_fixtures(
        cassie: &Cassie,
        postgres: &cassie::app::CassieSession,
        tenant: &cassie::app::CassieSession,
    ) {
        cassie
            .execute_sql(postgres, "CREATE DATABASE tenant_b", vec![])
            .unwrap();
        cassie
            .execute_sql(postgres, "CREATE SCHEMA reporting", vec![])
            .unwrap();
        for sql in [
        "CREATE TABLE public.shared_docs (id INT PRIMARY KEY, title TEXT)",
        "CREATE TABLE reporting.shared_docs (id INT PRIMARY KEY, title TEXT)",
        "CREATE TABLE reporting.orders (id INT PRIMARY KEY, doc_id INT REFERENCES reporting.shared_docs(id))",
        "CREATE VIEW reporting.shared_docs_view AS SELECT title FROM reporting.shared_docs",
    ] {
        cassie.execute_sql(postgres, sql, vec![]).unwrap();
    }

        cassie
            .execute_sql(tenant, "CREATE SCHEMA reporting", vec![])
            .unwrap();
        for sql in [
            "CREATE TABLE reporting.shared_docs (id INT PRIMARY KEY, title TEXT)",
            "CREATE VIEW reporting.shared_docs_view AS SELECT title FROM reporting.shared_docs",
        ] {
            cassie.execute_sql(tenant, sql, vec![]).unwrap();
        }
    }

    fn query_rows(
        cassie: &Cassie,
        session: &cassie::app::CassieSession,
        sql: &str,
    ) -> Vec<Vec<Value>> {
        cassie.execute_sql(session, sql, vec![]).unwrap().rows
    }

    #[test]
    fn should_bootstrap_default_database_with_public_schema_on_fresh_startup() {
        // Arrange
        use_local_storage();
        let path = data_dir("bootstrap_default_database");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();

            // Act
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            let databases = cassie
                .execute_sql(
                    &session,
                    "SELECT datname FROM pg_catalog.pg_database ORDER BY datname",
                    vec![],
                )
                .unwrap();
            let schemata = cassie
                .execute_sql(
                    &session,
                    "SELECT schema_name FROM information_schema.schemata ORDER BY schema_name",
                    vec![],
                )
                .unwrap();

            // Assert
            assert!(cassie.catalog.database_exists("postgres"));
            assert!(cassie.catalog.namespace_exists("postgres.public"));
            assert_eq!(
                databases.rows,
                vec![vec![Value::String("postgres".to_string())]]
            );
            assert_eq!(
                schemata.rows,
                vec![
                    vec![Value::String("information_schema".to_string())],
                    vec![Value::String("pg_catalog".to_string())],
                    vec![Value::String("public".to_string())],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_queries_for_missing_session_database() {
        // Arrange
        use_local_storage();
        let path = data_dir("missing_session_database");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", Some("missing_db".to_string()));

            // Act
            let error = cassie
                .execute_sql(&session, "SELECT 1", vec![])
                .expect_err("missing database should be rejected");

            // Assert
            let CassieError::CatalogObjectNotFound { kind, name } = error else {
                panic!("expected missing database error");
            };
            assert_eq!(kind, CatalogObjectKind::Database);
            assert_eq!(name, "missing_db");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_filter_catalog_views_plus_constraints_to_current_database() {
        // Arrange
        use_local_storage();
        let path = data_dir("catalog_filtering");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let postgres = cassie.create_session("tester", Some("postgres".to_string()));
        let tenant = cassie.create_session("tester", Some("tenant_b".to_string()));
        seed_catalog_filtering_fixtures(&cassie, &postgres, &tenant);

        // Act
        let tables = query_rows(
            &cassie,
            &postgres,
            "SELECT table_schema, table_name FROM information_schema.tables WHERE table_name IN ('shared_docs', 'shared_docs_view') ORDER BY table_schema, table_name",
        );
        let constraints = query_rows(
            &cassie,
            &postgres,
            "SELECT table_schema, table_name FROM information_schema.table_constraints WHERE table_name IN ('shared_docs', 'orders') ORDER BY table_schema, table_name",
        );
        let references = query_rows(
            &cassie,
            &postgres,
            "SELECT constraint_schema, unique_constraint_schema FROM information_schema.referential_constraints ORDER BY constraint_schema, unique_constraint_schema",
        );
        let tenant_tables = query_rows(
            &cassie,
            &tenant,
            "SELECT table_schema, table_name FROM information_schema.tables WHERE table_name = 'shared_docs' ORDER BY table_schema, table_name",
        );

        // Assert
        assert_eq!(
            tables,
            vec![
                vec![
                    Value::String("public".to_string()),
                    Value::String("shared_docs".to_string()),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::String("shared_docs".to_string()),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::String("shared_docs_view".to_string()),
                ],
            ]
        );
        assert_eq!(
            constraints,
            vec![
                vec![
                    Value::String("public".to_string()),
                    Value::String("shared_docs".to_string()),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::String("orders".to_string()),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::String("orders".to_string()),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::String("shared_docs".to_string()),
                ],
            ]
        );
        assert_eq!(
            references,
            vec![vec![
                Value::String("reporting".to_string()),
                Value::String("reporting".to_string()),
            ]]
        );
        assert_eq!(
            tenant_tables,
            vec![vec![
                Value::String("reporting".to_string()),
                Value::String("shared_docs".to_string()),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_restrict_pg_table_visibility_to_search_path() {
        // Arrange
        use_local_storage();
        let path = data_dir("pg_table_visibility");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE public.visible_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE reporting.visible_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(&session, "SET search_path = reporting", vec![])
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT relnamespace, pg_catalog.pg_table_is_visible(oid) FROM pg_catalog.pg_class WHERE relname = 'visible_docs' ORDER BY relnamespace",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::String("public".to_string()),
                    Value::Bool(false),
                ],
                vec![
                    Value::String("reporting".to_string()),
                    Value::Bool(true),
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/integration_sql_catalog.rs.
mod integration_sql_catalog {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::{canonical_relation_name, canonical_schema_name};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_execute_sql_query_after_catalog_hydration() {
        // Arrange
        use_local_storage();
        let path = data_dir("restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();

            let collection = canonical_relation_name("postgres", "public", "sql_hydration");
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
            };

            cassie.midge.create_collection(&collection, schema).unwrap();
            let _ = cassie
                .midge
                .put_document(
                    &collection,
                    None,
                    serde_json::json!({"title": "sql", "body": "hybrid path"}),
                )
                .unwrap();

            // Act
            drop(cassie);
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            let session = restarted.create_session("tester", None);
            let result = restarted
                .execute_sql(
                    &session,
                    "SELECT title FROM sql_hydration WHERE title = 'sql'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(result.columns[0].name, "title");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_persist_namespace_on_create_schema() {
        // Arrange
        use_local_storage();
        let path = data_dir("create_schema");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie
                .midge
                .ensure_families_ready()
                .expect("families ready");

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(&session, "CREATE SCHEMA analytics", vec![])
                .unwrap();

            // Assert
            assert_eq!(result.command, "CREATE SCHEMA");
            assert!(cassie.catalog.namespace_exists("analytics"));
            assert!(cassie
                .midge
                .list_namespaces()
                .iter()
                .any(|name| name == "analytics"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_rename_schema_through_sql() {
        // Arrange
        use_local_storage();
        let path = data_dir("rename_schema");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            let next_schema = canonical_schema_name("postgres", "reporting_archive");
            cassie
                .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "ALTER SCHEMA reporting RENAME TO reporting_archive",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.command, "ALTER SCHEMA");
            assert!(!cassie.catalog.namespace_exists("reporting"));
            assert!(cassie.catalog.namespace_exists("reporting_archive"));
            assert!(!cassie
                .midge
                .list_namespaces()
                .iter()
                .any(|name| name == "reporting"));
            assert!(cassie
                .midge
                .list_namespaces()
                .iter()
                .any(|name| name == &next_schema));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_drop_schema_through_sql() {
        // Arrange
        use_local_storage();
        let path = data_dir("drop_schema");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(&session, "DROP SCHEMA reporting", vec![])
                .unwrap();

            // Assert
            assert_eq!(result.command, "DROP SCHEMA");
            assert!(!cassie.catalog.namespace_exists("reporting"));
            assert!(!cassie
                .midge
                .list_namespaces()
                .iter()
                .any(|name| name == "reporting"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_ignore_duplicate_create_schema_when_if_not_exists_is_set() {
        // Arrange
        use_local_storage();
        let path = data_dir("create_schema_if_not_exists");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.midge.create_namespace("analytics").unwrap();

            let initial = cassie.midge.list_namespaces();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(&session, "CREATE SCHEMA IF NOT EXISTS analytics", vec![])
                .unwrap();

            // Assert
            assert_eq!(result.command, "CREATE SCHEMA");
            let namespaced = cassie.midge.list_namespaces();
            assert_eq!(namespaced.len(), initial.len());
            assert!(namespaced.iter().any(|name| name == "analytics"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_rename_column_through_sql() {
        // Arrange
        use_local_storage();
        let path = data_dir("rename_column");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            let relation = canonical_relation_name("postgres", "public", "rename_column_docs");
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE rename_column_docs (id TEXT, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    &relation,
                    Some("d1".to_string()),
                    serde_json::json!({"id": "d1", "title": "alpha"}),
                )
                .unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE rename_column_docs RENAME COLUMN title TO headline",
                    vec![],
                )
                .unwrap();
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT id, headline FROM rename_column_docs ORDER BY id",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.command, "ALTER TABLE");
            assert_eq!(selected.rows.len(), 1);
            assert_eq!(selected.rows[0][0], Value::String("d1".to_string()));
            assert_eq!(selected.rows[0][1], Value::String("alpha".to_string()));
            let schema = cassie
                .catalog
                .get_schema(&relation)
                .expect("schema should exist");
            assert!(schema.fields.iter().any(|field| field.name == "headline"));
            assert!(!schema.fields.iter().any(|field| field.name == "title"));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/role_authorization.rs.
mod role_authorization {
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use reqwest::StatusCode;
    use tokio_postgres::NoTls;

    use super::support_sql as support;
    use support::*;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    #[test]
    fn should_enforce_read_only_access_for_authenticated_non_admin_roles() {
        // Arrange
        use_local_storage();
        let path = data_dir("statement_families");
        let config = CassieRuntimeConfig {
            password: "sa-secret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
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
        use_local_storage();
        let path = data_dir("rest");
        let config = CassieRuntimeConfig {
            password: "sa-secret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");
        let admin = cassie
            .authenticate_role("root", Some("sa-secret"), None)
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
        use_local_storage();
        let path = data_dir("pgwire");
        let config = CassieRuntimeConfig {
            password: "sa-secret".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
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
}

// Formerly tests/role_database_copy_boundaries.rs.
mod role_database_copy_boundaries {
    use super::support_pgwire as support;

    use cassie::app::Cassie;
    use tokio::io::AsyncWriteExt;

    fn database_copy_error(test_name: &str, sql: &str) -> Vec<(char, String)> {
        support::use_local_storage();
        let path = support::data_dir(test_name);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let admin = cassie
            .authenticate_role("root", Some("postgres"), None)
            .expect("admin");
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE image_reader LOGIN PASSWORD 'reader-secret'",
                vec![],
            )
            .expect("create reader");
        cassie
            .execute_sql(&admin, "CREATE DATABASE denied_copy", vec![])
            .expect("create denied database");
        cassie
            .execute_sql(
                &admin,
                "GRANT CONNECT ON DATABASE postgres TO image_reader",
                vec![],
            )
            .expect("grant scoped database access");
        cassie
            .execute_sql(
                &admin,
                "GRANT CONNECT ON DATABASE denied_copy TO image_reader",
                vec![],
            )
            .expect("grant target database access");
        assert!(cassie
            .catalog
            .get_role("image_reader")
            .is_some_and(|role| role.can_access_database("denied_copy")));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let fields = runtime.block_on(async {
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            support::complete_startup_as(
                &mut reader,
                &mut write_half,
                "image_reader",
                "postgres",
                "reader-secret",
            )
            .await;
            write_half
                .write_all(&support::simple_query_frame(sql))
                .await
                .expect("write database image query");
            write_half
                .flush()
                .await
                .expect("flush database image query");
            let frames = support::read_frames_until_ready(&mut reader).await;
            let error = frames
                .iter()
                .find(|frame| frame.0 == b'E')
                .expect("insufficient privilege error");
            let fields = support::parse_error_fields(&error.1);
            server.stop().await;
            fields
        });
        let _ = std::fs::remove_dir_all(path);
        fields
    }

    #[test]
    fn should_reject_non_admin_database_backup_even_with_connect_grant() {
        // Arrange
        let sql = "BACKUP DATABASE denied_copy TO STDOUT";

        // Act
        let fields = database_copy_error("role-database-backup", sql);

        // Assert
        assert!(fields
            .iter()
            .any(|(kind, value)| *kind == 'C' && value == "42501"));
    }

    #[test]
    fn should_reject_non_admin_database_restore_even_with_connect_grant() {
        // Arrange
        let sql = "RESTORE DATABASE denied_copy FROM STDIN";

        // Act
        let fields = database_copy_error("role-database-restore", sql);

        // Assert
        assert!(fields
            .iter()
            .any(|(kind, value)| *kind == 'C' && value == "42501"));
    }

    #[test]
    fn should_document_admin_only_database_image_authorization() {
        // Arrange
        let contract = include_str!("../docs/postgres-compatibility.md");

        // Act
        let authorization = contract
            .split_once("## Database Image Authorization")
            .map(|(_, section)| section)
            .expect("database-image authorization contract");

        // Assert
        assert!(authorization.contains("admin-only"));
        assert!(authorization.contains("GRANT CONNECT"));
        assert!(authorization.contains("defense-in-depth"));
        assert!(authorization.contains("not a regression"));
    }
}
// Formerly tests/role_statement_boundaries.rs.
mod role_statement_boundaries {
    use cassie::app::{Cassie, CassieError, CassieSession};

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn assert_insufficient_privilege(result: &Result<cassie::executor::QueryResult, CassieError>) {
        assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
    }

    fn with_roles(test_name: &str, test: impl FnOnce(&Cassie, &CassieSession, &CassieSession)) {
        use_local_storage();
        let path = data_dir(test_name);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let admin = cassie
            .authenticate_role("root", Some("postgres"), None)
            .expect("admin");
        cassie
            .execute_sql(
                &admin,
                "CREATE TABLE role_statement_rows (title TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &admin,
                "CREATE ROLE statement_reader LOGIN PASSWORD 'reader-secret'",
                vec![],
            )
            .expect("create reader");
        let reader = cassie
            .authenticate_role("statement_reader", Some("reader-secret"), None)
            .expect("reader");
        test(&cassie, &admin, &reader);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_allow_read_only_role_to_execute_statements_given_select_show_set_and_transaction_families(
    ) {
        // Arrange
        with_roles("role-read-only-statements", |cassie, _admin, reader| {
            let statements = [
                "SELECT title FROM role_statement_rows",
                "SHOW search_path",
                "SET search_path TO public",
                "BEGIN",
                "ROLLBACK",
            ];

            // Act
            let results = statements.map(|sql| cassie.execute_sql(reader, sql, vec![]));

            // Assert
            assert!(results.iter().all(Result::is_ok));
        });
    }

    #[test]
    fn should_reject_read_only_role_explain_for_an_insert_statement() {
        // Arrange
        with_roles("role-explain-insert", |cassie, _admin, reader| {
            // Act
            let result = cassie.execute_sql(
                reader,
                "EXPLAIN INSERT INTO role_statement_rows (title) VALUES ('blocked')",
                vec![],
            );

            // Assert
            assert_insufficient_privilege(&result);
        });
    }

    #[test]
    fn should_reject_read_only_role_explain_for_a_nested_mutating_statement() {
        // Arrange
        with_roles("role-nested-explain-insert", |cassie, _admin, reader| {
            // Act
            let result = cassie.execute_sql(
                reader,
                "EXPLAIN EXPLAIN INSERT INTO role_statement_rows (title) VALUES ('blocked')",
                vec![],
            );

            // Assert
            assert_insufficient_privilege(&result);
        });
    }

    #[test]
    fn should_reject_read_only_role_copy_from_stdin() {
        // Arrange
        with_roles("role-copy-from", |cassie, _admin, reader| {
            // Act
            let result = cassie.execute_sql(
                reader,
                "COPY role_statement_rows FROM STDIN WITH (FORMAT csv)",
                vec![],
            );

            // Assert
            assert_insufficient_privilege(&result);
        });
    }

    #[test]
    fn should_reject_read_only_role_copy_to_stdout() {
        // Arrange
        with_roles("role-copy-to", |cassie, _admin, reader| {
            // Act
            let result = cassie.execute_sql(
                reader,
                "COPY role_statement_rows TO STDOUT WITH (FORMAT csv)",
                vec![],
            );

            // Assert
            assert_insufficient_privilege(&result);
        });
    }

    #[test]
    fn should_allow_admin_role_to_execute_mutating_explain_statements() {
        // Arrange
        with_roles("role-admin-explain", |cassie, admin, _reader| {
            // Act
            let result = cassie.execute_sql(
                admin,
                "EXPLAIN INSERT INTO role_statement_rows (title) VALUES ('allowed')",
                vec![],
            );

            // Assert
            assert!(result.is_ok());
        });
    }
}

// Formerly tests/schema_epoch_safety.rs.
mod schema_epoch_safety {
    use cassie::app::Cassie;
    use cassie::midge::adapter::StorageFamily;
    use std::path::PathBuf;
    use std::sync::Arc;
    use uuid::Uuid;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn data_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cassie-schema-epoch-{name}-{}", Uuid::new_v4()))
    }

    fn compile_physical_plan(
        cassie: &Cassie,
        sql: &str,
    ) -> Arc<cassie::planner::physical::PhysicalPlan> {
        cassie
            .compile_sql_physical_plan_for_diagnostics(sql)
            .unwrap()
    }

    fn scalar_index_sidecars(cassie: &Cassie, collection: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
        let relation_id = cassie
            .midge
            .collection_metadata(collection)
            .unwrap()
            .expect("collection metadata")
            .storage_id;
        let prefix = cassie::midge::adapter::Midge::scalar_index_collection_prefix_for_diagnostics(
            relation_id,
        );
        cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, prefix.as_slice())
            .unwrap()
    }

    #[test]
    fn should_defer_drop_table_physical_cleanup_until_pinned_schema_epoch_drains() {
        // Arrange
        use_local_storage();
        let path = data_dir("drop_table_deferred_cleanup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE epoch_drop_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO epoch_drop_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            let collection = cassie
                .catalog
                .get_schema("epoch_drop_docs")
                .expect("catalog collection")
                .collection;
            let pinned = cassie.begin_schema_epoch_guard_for_diagnostics();

            // Act
            cassie
                .execute_sql(&session, "DROP TABLE epoch_drop_docs", vec![])
                .unwrap();
            let new_query =
                cassie.execute_sql(&session, "SELECT title FROM epoch_drop_docs", vec![]);
            let rows_while_pinned = cassie.midge.scan_documents(&collection).unwrap();
            cassie
                .run_deferred_schema_cleanup_for_diagnostics()
                .unwrap();
            let rows_after_pinned_cleanup = cassie.midge.scan_documents(&collection).unwrap();
            drop(pinned);
            cassie
                .run_deferred_schema_cleanup_for_diagnostics()
                .unwrap();
            let rows_after_drain = cassie.midge.scan_documents(&collection);

            // Assert
            assert!(new_query.is_err());
            assert_eq!(rows_while_pinned.len(), 1);
            assert_eq!(
                rows_while_pinned[0].payload["title"],
                serde_json::json!("alpha")
            );
            assert_eq!(rows_after_pinned_cleanup.len(), 1);
            assert!(rows_after_drain.is_err());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_defer_drop_index_sidecar_cleanup_until_pinned_schema_epoch_drains() {
        // Arrange
        use_local_storage();
        let path = data_dir("drop_index_deferred_cleanup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE epoch_drop_index_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO epoch_drop_index_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            cassie
            .execute_sql(
                &session,
                "CREATE INDEX epoch_drop_title_idx ON epoch_drop_index_docs USING btree (title)",
                vec![],
            )
            .unwrap();
            let collection = cassie
                .catalog
                .get_schema("epoch_drop_index_docs")
                .expect("catalog collection")
                .collection;
            let stored_index_name = cassie
                .catalog
                .get_index(&collection, "epoch_drop_title_idx")
                .expect("catalog index")
                .name;
            let sidecars_before = scalar_index_sidecars(&cassie, &collection);
            let pinned = cassie.begin_schema_epoch_guard_for_diagnostics();

            // Act
            cassie
                .execute_sql(
                    &session,
                    "DROP INDEX epoch_drop_title_idx ON epoch_drop_index_docs",
                    vec![],
                )
                .unwrap();
            let new_plan = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM epoch_drop_index_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            cassie
                .run_deferred_schema_cleanup_for_diagnostics()
                .unwrap();
            let sidecars_while_pinned = scalar_index_sidecars(&cassie, &collection);
            let stored_index_while_pinned = cassie
                .midge
                .get_index(&collection, &stored_index_name)
                .unwrap();
            drop(pinned);
            cassie
                .run_deferred_schema_cleanup_for_diagnostics()
                .unwrap();
            let sidecars_after_drain = scalar_index_sidecars(&cassie, &collection);
            let stored_index_after_drain = cassie
                .midge
                .get_index(&collection, &stored_index_name)
                .unwrap();

            // Assert
            assert!(!sidecars_before.is_empty());
            let cassie::types::Value::String(plan) = &new_plan.rows[0][0] else {
                panic!("expected explain text");
            };
            assert!(plan.contains("index=none"), "plan={plan}");
            assert!(cassie
                .catalog
                .get_index(&collection, "epoch_drop_title_idx")
                .is_none());
            assert!(!sidecars_while_pinned.is_empty());
            assert!(stored_index_while_pinned.is_some());
            assert!(sidecars_after_drain.is_empty());
            assert!(stored_index_after_drain.is_none());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_defer_drop_view_metadata_cleanup_until_pinned_schema_epoch_drains() {
        // Arrange
        use_local_storage();
        let path = data_dir("drop_view_deferred_cleanup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE epoch_view_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE VIEW epoch_view_ready AS SELECT title FROM epoch_view_docs",
                    vec![],
                )
                .unwrap();
            let view = cassie
                .catalog
                .get_view("epoch_view_ready")
                .expect("catalog view")
                .name;
            let pinned = cassie.begin_schema_epoch_guard_for_diagnostics();

            // Act
            cassie
                .execute_sql(&session, "DROP VIEW epoch_view_ready", vec![])
                .unwrap();
            let new_query =
                cassie.execute_sql(&session, "SELECT title FROM epoch_view_ready", vec![]);
            let view_while_pinned = cassie.midge.get_view(&view).unwrap();
            cassie
                .run_deferred_schema_cleanup_for_diagnostics()
                .unwrap();
            let view_after_pinned_cleanup = cassie.midge.get_view(&view).unwrap();
            drop(pinned);
            cassie
                .run_deferred_schema_cleanup_for_diagnostics()
                .unwrap();
            let view_after_drain = cassie.midge.get_view(&view).unwrap();

            // Assert
            assert!(new_query.is_err());
            assert!(view_while_pinned.is_some());
            assert!(view_after_pinned_cleanup.is_some());
            assert!(view_after_drain.is_none());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_finish_pending_schema_cleanup_on_startup_without_rehydrating_dropped_table() {
        // Arrange
        use_local_storage();
        let path = data_dir("startup_pending_cleanup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE epoch_restart_drop_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO epoch_restart_drop_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            let collection = cassie
                .catalog
                .get_schema("epoch_restart_drop_docs")
                .expect("catalog collection")
                .collection;
            let pinned = cassie.begin_schema_epoch_guard_for_diagnostics();
            cassie
                .execute_sql(&session, "DROP TABLE epoch_restart_drop_docs", vec![])
                .unwrap();
            let rows_before_restart = cassie.midge.scan_documents(&collection).unwrap();
            drop(pinned);
            drop(cassie);

            // Act
            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            let session = restarted.create_session("tester", None);
            let new_query = restarted.execute_sql(
                &session,
                "SELECT title FROM epoch_restart_drop_docs",
                vec![],
            );
            let physical_rows = restarted.midge.scan_documents(&collection);

            // Assert
            assert_eq!(rows_before_restart.len(), 1);
            assert!(new_query.is_err());
            assert!(physical_rows.is_err());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_saved_plan_with_schema_snapshot_after_column_rename() {
        // Arrange
        use_local_storage();
        let path = data_dir("saved_plan_column_rename");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE epoch_rename_docs (id TEXT, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO epoch_rename_docs (id, title) VALUES ('d1', 'alpha')",
                    vec![],
                )
                .unwrap();
            let saved_plan = compile_physical_plan(
                &cassie,
                "SELECT id, title FROM epoch_rename_docs ORDER BY id",
            );

            // Act
            cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE epoch_rename_docs RENAME COLUMN title TO headline",
                    vec![],
                )
                .unwrap();
            let saved = cassie
                .execute_physical_plan_for_diagnostics(&session, &saved_plan)
                .unwrap();
            let current = cassie
                .execute_sql(
                    &session,
                    "SELECT id, headline FROM epoch_rename_docs ORDER BY id",
                    vec![],
                )
                .unwrap();
            let old_name = cassie.execute_sql(
                &session,
                "SELECT id, title FROM epoch_rename_docs ORDER BY id",
                vec![],
            );

            // Assert
            assert_eq!(saved.columns[1].name, "title");
            assert_eq!(
                saved.rows[0][1],
                cassie::types::Value::String("alpha".to_string())
            );
            assert_eq!(current.columns[1].name, "headline");
            assert_eq!(
                current.rows[0][1],
                cassie::types::Value::String("alpha".to_string())
            );
            assert!(old_name.is_err());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_saved_plan_with_schema_snapshot_after_column_drop() {
        // Arrange
        use_local_storage();
        let path = data_dir("saved_plan_column_drop");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE epoch_drop_column_docs (id TEXT, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO epoch_drop_column_docs (id, title) VALUES ('d1', 'alpha')",
                    vec![],
                )
                .unwrap();
            let saved_plan = compile_physical_plan(
                &cassie,
                "SELECT id, title FROM epoch_drop_column_docs ORDER BY id",
            );

            // Act
            cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE epoch_drop_column_docs DROP COLUMN title",
                    vec![],
                )
                .unwrap();
            let saved = cassie
                .execute_physical_plan_for_diagnostics(&session, &saved_plan)
                .unwrap();
            let current = cassie
                .execute_sql(
                    &session,
                    "SELECT id FROM epoch_drop_column_docs ORDER BY id",
                    vec![],
                )
                .unwrap();
            let dropped = cassie.execute_sql(
                &session,
                "SELECT title FROM epoch_drop_column_docs ORDER BY id",
                vec![],
            );

            // Assert
            assert_eq!(saved.columns[1].name, "title");
            assert_eq!(
                saved.rows[0][1],
                cassie::types::Value::String("alpha".to_string())
            );
            assert_eq!(current.columns.len(), 1);
            assert!(dropped.is_err());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_keep_saved_wildcard_plan_on_schema_snapshot_after_column_add() {
        // Arrange
        use_local_storage();
        let path = data_dir("saved_plan_column_add");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE epoch_add_column_docs (id TEXT, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO epoch_add_column_docs (id, title) VALUES ('d1', 'alpha')",
                    vec![],
                )
                .unwrap();
            let saved_plan =
                compile_physical_plan(&cassie, "SELECT * FROM epoch_add_column_docs ORDER BY id");

            // Act
            cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE epoch_add_column_docs ADD COLUMN status TEXT",
                    vec![],
                )
                .unwrap();
            let saved = cassie
                .execute_physical_plan_for_diagnostics(&session, &saved_plan)
                .unwrap();
            let current = cassie
                .execute_sql(
                    &session,
                    "SELECT * FROM epoch_add_column_docs ORDER BY id",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                saved
                    .columns
                    .iter()
                    .map(|column| column.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["id", "title"]
            );
            assert_eq!(
                current
                    .columns
                    .iter()
                    .map(|column| column.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["id", "title", "status"]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/schema_operation_recovery.rs.
mod schema_operation_recovery {
    use cassie::app::Cassie;
    use cassie::midge::adapter::{
        set_collection_drop_failure_point, set_collection_rename_failure_point,
        set_field_add_failure_point, set_field_drop_failure_point, set_field_rename_failure_point,
        set_index_drop_failure_point, Midge, StorageFamily,
    };
    use cassie::types::{DataType, FieldSchema, Schema};

    use super::support_sql as support;
    use support::{canonical_test_collection, data_dir, use_local_storage};

    static COLLECTION_DROP_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static COLLECTION_RENAME_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static INDEX_DROP_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static FIELD_ADD_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn should_replay_add_column_derived_state_after_schema_commit_interruption() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = FIELD_ADD_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("schema_operation_field_add_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE field_add_recovery (keep TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO field_add_recovery (keep) VALUES ('alpha')",
                vec![],
            )
            .expect("seed row");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX field_add_recovery_cover ON field_add_recovery USING column (keep)",
                vec![],
            )
            .expect("create column index");
        let collection = canonical_test_collection(&cassie, "field_add_recovery");
        // Act
        set_field_add_failure_point(true);
        assert!(cassie
            .midge
            .alter_collection_add_column(
                &collection,
                FieldSchema {
                    name: "added".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            )
            .is_err());
        let schema_after_interrupt = cassie
            .midge
            .collection_schema(&collection)
            .expect("schema remains durable");
        assert!(schema_after_interrupt
            .fields
            .iter()
            .any(|field| field.name == "added"));
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay field add");
        let restarted_session = restarted.create_session("tester", None);
        let result = restarted
            .execute_sql(
                &restarted_session,
                "SELECT keep, added FROM field_add_recovery",
                vec![],
            )
            .expect("query after field-add replay");
        let root = restarted
            .midge
            .root_hash(&collection)
            .expect("read rebuilt projection root")
            .expect("projection root after replay");
        let generation = restarted
            .midge
            .collection_generation(&collection)
            .expect("read collection generation");
        let metadata = restarted
            .midge
            .get_column_batch_metadata(&collection, "field_add_recovery_cover")
            .expect("read rebuilt column batches")
            .expect("column batches after replay");
        drop(restarted);
        let restarted_again = Cassie::new_with_data_dir(&path).expect("reopen Cassie again");
        restarted_again
            .startup()
            .expect("replay field add idempotently");

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                cassie::types::Value::String("alpha".to_string()),
                cassie::types::Value::Null,
            ]]
        );
        assert_eq!(generation, 2);
        assert_eq!(root.built_generation, generation);
        assert_eq!(metadata.built_generation, generation);
        assert!(restarted_again
            .midge
            .root_hash(&collection)
            .expect("read root after second restart")
            .is_some());

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_discard_rejected_collection_rename_intent_on_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("schema_operation_rejected_rename");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection("rename_rejected_source", schema.clone())
            .expect("create source collection");
        cassie
            .midge
            .create_collection("rename_rejected_target", schema)
            .expect("create target collection");
        cassie
            .midge
            .put_document(
                "rename_rejected_source",
                Some("source-row".to_string()),
                serde_json::json!({"title": "source"}),
            )
            .expect("seed source document");
        cassie
            .midge
            .put_document(
                "rename_rejected_target",
                Some("target-row".to_string()),
                serde_json::json!({"title": "target"}),
            )
            .expect("seed target document");

        // Act
        assert!(cassie
            .midge
            .rename_collection("rename_rejected_source", "rename_rejected_target")
            .is_err());
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("discard rejected rename intent");

        // Assert
        let source = restarted
            .midge
            .scan_documents("rename_rejected_source")
            .expect("scan source collection");
        let target = restarted
            .midge
            .scan_documents("rename_rejected_target")
            .expect("scan target collection");
        assert_eq!(source.len(), 1);
        assert_eq!(source[0].id, "source-row");
        assert_eq!(source[0].payload, serde_json::json!({"title": "source"}));
        assert_eq!(target.len(), 1);
        assert_eq!(target[0].id, "target-row");
        assert_eq!(target[0].payload, serde_json::json!({"title": "target"}));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_replay_drop_collection_cleanup_after_schema_commit() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = COLLECTION_DROP_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("schema_operation_drop_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection("drop_recovery", schema)
            .expect("create collection");
        cassie
            .midge
            .put_document(
                "drop_recovery",
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("seed document");
        cassie
            .midge
            .defer_drop_collection("drop_recovery", 0)
            .expect("defer collection drop");
        set_collection_drop_failure_point(true);

        // Act
        assert!(cassie
            .run_deferred_schema_cleanup_for_diagnostics()
            .is_err());
        let schema_after_interrupt = cassie.midge.collection_schema("drop_recovery");
        let generation_after_interrupt = cassie
            .midge
            .collection_generation("drop_recovery")
            .expect("read retained generation");
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay collection drop");
        assert!(restarted.midge.collection_schema("drop_recovery").is_none());
        assert_eq!(
            restarted
                .midge
                .collection_generation("drop_recovery")
                .unwrap(),
            0
        );
        drop(restarted);
        let restarted_again = Cassie::new_with_data_dir(&path).expect("reopen Cassie again");
        restarted_again
            .startup()
            .expect("replay collection drop idempotently");

        // Assert
        assert!(schema_after_interrupt.is_none());
        assert_eq!(generation_after_interrupt, 1);
        assert!(restarted_again
            .midge
            .collection_schema("drop_recovery")
            .is_none());

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_replay_drop_graph_collection_cleanup_without_orphaned_adjacency() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = COLLECTION_DROP_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("schema_operation_drop_graph_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE GRAPH drop_graph_recovery (NODES (label TEXT), EDGES (source TEXT))",
                vec![],
            )
            .expect("create graph");
        cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_graph_recovery_nodes (node_type, node_id, label) VALUES ('person', 'alice', 'Alice'), ('person', 'bob', 'Bob')",
            vec![],
        )
        .expect("seed graph nodes");
        cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_graph_recovery_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight, source) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1, 'direct')",
            vec![],
        )
        .expect("seed graph edge");
        let before_drop = cassie
        .execute_sql(
            &session,
            "SELECT edge_id FROM graph_neighbors('drop_graph_recovery', 'person', 'alice', 'out', 'knows', 10)",
            vec![],
        )
        .expect("read graph adjacency before drop");
        assert_eq!(before_drop.rows.len(), 1);
        let edge_collection = canonical_test_collection(&cassie, "drop_graph_recovery_edges");
        cassie
            .midge
            .defer_drop_collection(&edge_collection, 0)
            .expect("defer graph edge collection drop");
        set_collection_drop_failure_point(true);

        // Act
        assert!(cassie
            .run_deferred_schema_cleanup_for_diagnostics()
            .is_err());
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay graph collection drop");
        let restarted_session = restarted.create_session("tester", None);
        let after_drop = restarted
        .execute_sql(
            &restarted_session,
            "SELECT edge_id FROM graph_neighbors('drop_graph_recovery', 'person', 'alice', 'out', 'knows', 10)",
            vec![],
        )
        .expect("read graph adjacency after drop");
        drop(restarted);
        let restarted_again = Cassie::new_with_data_dir(&path).expect("reopen Cassie again");
        restarted_again
            .startup()
            .expect("replay graph collection drop idempotently");

        // Assert
        assert!(after_drop.rows.is_empty());
        let after_second_restart = restarted_again
        .execute_sql(
            &restarted_again.create_session("tester", None),
            "SELECT edge_id FROM graph_neighbors('drop_graph_recovery', 'person', 'alice', 'out', 'knows', 10)",
            vec![],
        )
        .expect("read graph adjacency after second restart");
        assert!(after_second_restart.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_replay_drop_index_cleanup_after_metadata_interrupt() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = INDEX_DROP_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("schema_operation_drop_index_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE drop_index_recovery (title TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO drop_index_recovery (title) VALUES ('alpha')",
                vec![],
            )
            .expect("seed row");
        cassie
        .execute_sql(
            &session,
            "CREATE INDEX drop_index_recovery_title_idx ON drop_index_recovery USING btree (title)",
            vec![],
        )
        .expect("create index");
        let collection = cassie
            .catalog
            .get_schema("drop_index_recovery")
            .expect("catalog collection")
            .collection;
        let relation_id = cassie
            .midge
            .collection_metadata(&collection)
            .expect("read collection metadata")
            .expect("collection metadata")
            .storage_id;
        let prefix = Midge::scalar_index_collection_prefix_for_diagnostics(relation_id);
        assert!(!cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("read scalar sidecars")
            .is_empty());
        cassie
            .midge
            .defer_drop_index("drop_index_recovery", "drop_index_recovery_title_idx", 0)
            .expect("defer index cleanup");
        set_index_drop_failure_point(true);

        // Act
        assert!(cassie
            .run_deferred_schema_cleanup_for_diagnostics()
            .is_err());
        assert!(cassie
            .midge
            .get_index(&collection, "drop_index_recovery_title_idx")
            .expect("read interrupted metadata")
            .is_some());
        assert!(cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("read cleaned scalar sidecars")
            .is_empty());
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay index cleanup");
        assert!(restarted
            .midge
            .get_index(&collection, "drop_index_recovery_title_idx")
            .expect("read removed metadata")
            .is_none());
        assert!(restarted
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("read removed scalar sidecars")
            .is_empty());
        drop(restarted);
        let restarted_again = Cassie::new_with_data_dir(&path).expect("reopen Cassie again");
        restarted_again
            .startup()
            .expect("replay index cleanup idempotently");

        // Assert
        assert!(restarted_again
            .midge
            .get_index(&collection, "drop_index_recovery_title_idx")
            .expect("read removed metadata after second restart")
            .is_none());

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_replay_collection_rename_data_after_schema_commit_interruption() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = COLLECTION_RENAME_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("schema_operation_rename_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection("rename_recovery_before", schema)
            .expect("create collection");
        cassie
            .midge
            .put_document(
                "rename_recovery_before",
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("seed document");

        // Act
        set_collection_rename_failure_point(true);
        assert!(cassie
            .midge
            .rename_collection("rename_recovery_before", "rename_recovery_after")
            .is_err());
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay rename");

        // Assert
        let documents = restarted
            .midge
            .scan_documents("rename_recovery_after")
            .expect("scan renamed documents");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].id, "doc-1");
        assert_eq!(documents[0].payload, serde_json::json!({"title": "alpha"}));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_destination_write_when_collection_rename_replays() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = COLLECTION_RENAME_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("schema_operation_rename_destination_write");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection("rename_destination_before", schema)
            .expect("create collection");
        cassie
            .midge
            .put_document(
                "rename_destination_before",
                Some("shared-row".to_string()),
                serde_json::json!({"title": "before"}),
            )
            .expect("seed source document");

        // Act
        set_collection_rename_failure_point(true);
        assert!(cassie
            .midge
            .rename_collection("rename_destination_before", "rename_destination_after")
            .is_err());
        cassie
            .midge
            .put_document(
                "rename_destination_after",
                Some("shared-row".to_string()),
                serde_json::json!({"title": "after"}),
            )
            .expect("write to committed destination collection");
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted
            .startup()
            .expect("replay rename without data loss");

        // Assert
        let documents = restarted
            .midge
            .scan_documents("rename_destination_after")
            .expect("scan renamed documents");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].id, "shared-row");
        assert_eq!(documents[0].payload, serde_json::json!({"title": "after"}));
        assert!(restarted
            .midge
            .collection_schema("rename_destination_before")
            .is_none());

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_replay_field_rename_data_after_schema_commit_interruption() {
        // Arrange
        use_local_storage();
        let path = data_dir("schema_operation_field_rename_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "before".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection("field_rename_recovery", schema)
            .expect("create collection");
        cassie
            .midge
            .put_document(
                "field_rename_recovery",
                Some("doc-1".to_string()),
                serde_json::json!({"before": "alpha"}),
            )
            .expect("seed document");

        // Act
        set_field_rename_failure_point(true);
        assert!(cassie
            .midge
            .alter_collection_rename_column("field_rename_recovery", "before", "after")
            .is_err());
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay field rename");

        // Assert
        let documents = restarted
            .midge
            .scan_documents("field_rename_recovery")
            .expect("scan renamed documents");
        assert_eq!(documents[0].payload, serde_json::json!({"after": "alpha"}));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_replay_field_drop_data_after_schema_commit_interruption() {
        // Arrange
        use_local_storage();
        let path = data_dir("schema_operation_field_drop_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "keep".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "remove".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection("field_drop_recovery", schema)
            .expect("create collection");
        cassie
            .midge
            .put_document(
                "field_drop_recovery",
                Some("doc-1".to_string()),
                serde_json::json!({"keep": "alpha", "remove": "discard"}),
            )
            .expect("seed document");

        // Act
        set_field_drop_failure_point(true);
        assert!(cassie
            .midge
            .alter_collection_drop_column("field_drop_recovery", "remove")
            .is_err());
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("replay field drop");

        // Assert
        let documents = restarted
            .midge
            .scan_documents("field_drop_recovery")
            .expect("scan dropped-field documents");
        assert_eq!(documents[0].payload, serde_json::json!({"keep": "alpha"}));

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/schema_scope_storage.rs.
mod schema_scope_storage {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    #[test]
    fn should_isolate_duplicate_relation_names_across_databases_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("restart_isolation");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            {
                let cassie = Cassie::new_with_data_dir(&path).unwrap();
                cassie.startup().unwrap();
                let postgres = cassie.create_session("tester", Some("postgres".to_string()));
                cassie
                    .execute_sql(&postgres, "CREATE DATABASE tenant_b", vec![])
                    .unwrap();
                cassie
                    .execute_sql(&postgres, "CREATE SCHEMA reporting", vec![])
                    .unwrap();
                cassie
                    .execute_sql(
                        &postgres,
                        "CREATE TABLE reporting.docs (title TEXT)",
                        vec![],
                    )
                    .unwrap();
                cassie
                    .execute_sql(
                        &postgres,
                        "INSERT INTO reporting.docs (title) VALUES ('postgres-row')",
                        vec![],
                    )
                    .unwrap();

                let tenant = cassie.create_session("tester", Some("tenant_b".to_string()));
                cassie
                    .execute_sql(&tenant, "CREATE SCHEMA reporting", vec![])
                    .unwrap();
                cassie
                    .execute_sql(&tenant, "CREATE TABLE reporting.docs (title TEXT)", vec![])
                    .unwrap();
                cassie
                    .execute_sql(
                        &tenant,
                        "INSERT INTO reporting.docs (title) VALUES ('tenant-row')",
                        vec![],
                    )
                    .unwrap();

                cassie.shutdown();
            }

            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            let postgres = restarted.create_session("tester", Some("postgres".to_string()));
            let tenant = restarted.create_session("tester", Some("tenant_b".to_string()));

            // Act
            let postgres_rows = restarted
                .execute_sql(
                    &postgres,
                    "SELECT title FROM reporting.docs ORDER BY title",
                    vec![],
                )
                .unwrap();
            let tenant_rows = restarted
                .execute_sql(
                    &tenant,
                    "SELECT title FROM reporting.docs ORDER BY title",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(postgres_rows.rows.len(), 1);
            assert_eq!(tenant_rows.rows.len(), 1);
            assert_eq!(
                postgres_rows.rows[0][0],
                Value::String("postgres-row".to_string())
            );
            assert_eq!(
                tenant_rows.rows[0][0],
                Value::String("tenant-row".to_string())
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_rewrite_collection_sidecars_when_schema_is_renamed() {
        // Arrange
        use_local_storage();
        let path = data_dir("rename_schema_sidecars");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        let current = canonical_relation_name("postgres", "reporting", "metrics");
        let next = canonical_relation_name("postgres", "reporting_archive", "metrics");

        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE reporting.metrics (id INT PRIMARY KEY, status TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for id in 0..8 {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO reporting.metrics (id, status, body) VALUES ({id}, 'active', 'same')"
                    ),
                    vec![],
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_reporting_metrics_status ON reporting.metrics USING column (status, body) WITH (segment_size = 8)",
                vec![],
            )
            .unwrap();
        let current_index = cassie
            .catalog
            .list_indexes(&current)
            .into_iter()
            .find(|index| index.kind == cassie::catalog::IndexKind::Column)
            .map(|index| index.name)
            .expect("column index should be registered");
        assert!(cassie.midge.root_hash(&current).unwrap().is_some());
        assert!(cassie
            .midge
            .get_column_batch_metadata(&current, &current_index)
            .unwrap()
            .is_some());

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER SCHEMA reporting RENAME TO reporting_archive",
                vec![],
            )
            .unwrap();
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT id FROM reporting_archive.metrics ORDER BY id",
                vec![],
            )
            .unwrap();
        let next_index = cassie
            .catalog
            .list_indexes(&next)
            .into_iter()
            .find(|index| index.kind == cassie::catalog::IndexKind::Column)
            .map(|index| index.name)
            .expect("renamed column index should be registered");

        // Assert
        assert_eq!(rows.rows.len(), 8);
        assert!(cassie.midge.root_hash(&current).unwrap().is_none());
        assert!(cassie.midge.root_hash(&next).unwrap().is_some());
        assert!(cassie
            .midge
            .get_column_batch_metadata(&current, &current_index)
            .unwrap()
            .is_none());
        assert!(cassie
            .midge
            .get_column_batch_metadata(&next, &next_index)
            .unwrap()
            .is_some());
        assert!(cassie
            .execute_sql(
                &session,
                "SELECT id FROM reporting.metrics ORDER BY id",
                vec![],
            )
            .is_err());

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_dropping_non_empty_schema_without_cascade() {
        // Arrange
        use_local_storage();
        let path = data_dir("drop_non_empty_schema");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", Some("postgres".to_string()));
            cassie
                .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
                .unwrap();
            cassie
                .execute_sql(&session, "CREATE TABLE reporting.docs (title TEXT)", vec![])
                .unwrap();

            // Act
            let error = cassie
                .execute_sql(&session, "DROP SCHEMA reporting", vec![])
                .expect_err("non-empty schema should be rejected");

            // Assert
            assert!(error
                .to_string()
                .contains("namespace 'postgres.reporting' is not empty"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_dropping_current_or_non_empty_database() {
        // Arrange
        use_local_storage();
        let path = data_dir("drop_database_guards");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let postgres = cassie.create_session("tester", Some("postgres".to_string()));
            cassie
                .execute_sql(&postgres, "CREATE DATABASE tenant_b", vec![])
                .unwrap();
            let tenant = cassie.create_session("tester", Some("tenant_b".to_string()));
            cassie
                .execute_sql(&tenant, "CREATE TABLE public.docs (title TEXT)", vec![])
                .unwrap();

            // Act
            let current_error = cassie
                .execute_sql(&tenant, "DROP DATABASE tenant_b", vec![])
                .expect_err("current database should be protected");
            let non_empty_error = cassie
                .execute_sql(&postgres, "DROP DATABASE tenant_b", vec![])
                .expect_err("non-empty database should be rejected");

            // Assert
            assert!(current_error
                .to_string()
                .contains("cannot drop the currently open database 'tenant_b'"));
            assert!(non_empty_error
                .to_string()
                .contains("database 'tenant_b' is not empty"));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/session_settings.rs.
mod session_settings {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::types::Value;

    use support::*;

    fn cassie_and_session(label: &str) -> (Cassie, cassie::CassieSession, String) {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("root", Some("postgres".to_string()));
        (cassie, session, path)
    }

    #[test]
    fn should_use_one_registry_for_setting_reads() {
        // Arrange
        let (cassie, session, path) = cassie_and_session("shared_registry");

        // Act
        cassie
            .execute_sql(&session, "SET application_name='DBeaver 26.1.3'", vec![])
            .expect("set application name");
        let shown = cassie
            .execute_sql(&session, "SHOW application_name", vec![])
            .expect("show application name");
        let current = cassie
            .execute_sql(
                &session,
                "SELECT current_setting('application_name')",
                vec![],
            )
            .expect("current_setting");
        let catalog = cassie
            .execute_sql(
                &session,
                "SELECT setting FROM pg_catalog.pg_settings WHERE name = 'application_name'",
                vec![],
            )
            .expect("pg_settings");

        // Assert
        assert_eq!(
            shown.rows[0][0],
            Value::String("DBeaver 26.1.3".to_string())
        );
        assert_eq!(
            current.rows[0][0],
            Value::String("DBeaver 26.1.3".to_string())
        );
        assert_eq!(catalog.rows, current.rows);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_replay_exact_pgadmin_initialization_settings() {
        // Arrange
        let (cassie, session, path) = cassie_and_session("pgadmin_init");

        // Act
        cassie
            .execute_sql(&session, "SET DateStyle=ISO", vec![])
            .expect("DateStyle");
        cassie
            .execute_sql(&session, "SET client_min_messages=notice", vec![])
            .expect("messages");
        let configured = cassie.execute_sql(
            &session,
            "SELECT set_config('bytea_output','hex',false) FROM pg_show_all_settings() WHERE name='bytea_output'",
            vec![],
        ).expect("bytea_output initialization");
        cassie
            .execute_sql(&session, "SET client_encoding='UTF8'", vec![])
            .expect("encoding");

        // Assert
        assert_eq!(
            configured.rows,
            vec![vec![Value::String("hex".to_string())]]
        );
        assert_eq!(session.setting("datestyle").unwrap(), "ISO, MDY");
        assert_eq!(session.setting("client_min_messages").unwrap(), "notice");
        assert_eq!(session.setting("client_encoding").unwrap(), "UTF8");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_accept_postgres_set_time_zone_syntax() {
        // Arrange
        let (cassie, session, path) = cassie_and_session("set_time_zone");

        // Act
        let result = cassie.execute_sql(&session, "SET TIME ZONE 'UTC'", vec![]);

        // Assert
        result.expect("set PostgreSQL time zone syntax");
        assert_eq!(session.setting("timezone").unwrap(), "UTC");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_invalid_settings_with_22023() {
        // Arrange
        let (cassie, session, path) = cassie_and_session("invalid_values");

        // Act
        let fixed = cassie.execute_sql(&session, "SET TimeZone='America/New_York'", vec![]);
        let unknown = cassie.execute_sql(&session, "SET made_up_setting='yes'", vec![]);

        // Assert
        for error in [fixed.unwrap_err(), unknown.unwrap_err()] {
            assert!(error.to_string().contains("parameter"));
        }
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_expose_client_version_functions() {
        // Arrange
        let (cassie, session, path) = cassie_and_session("version_identity");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT version(), pg_catalog.version(), cassie_version()",
                vec![],
            )
            .expect("version functions");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert!(
            matches!(&result.rows[0][0], Value::String(value) if value.starts_with("PostgreSQL 16.0 compatible Cassie"))
        );
        assert_eq!(result.rows[0][0], result.rows[0][1]);
        assert_eq!(
            result.rows[0][2],
            Value::String(env!("CARGO_PKG_VERSION").to_string())
        );
        let _ = std::fs::remove_dir_all(path);
    }
}
// Formerly tests/views.rs.
mod views {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn use_local_storage() {
        if std::env::var("CASSIE_EMBEDDINGS_PROVIDER").is_err() {
            std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "fallback");
        }
    }

    fn data_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cassie-view-{label}-{}", Uuid::new_v4()))
    }

    fn seed_view_docs(cassie: &Cassie, collection: &str) {
        let collection = canonical_relation_name("postgres", "public", collection);
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(&collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            &collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        cassie
            .midge
            .put_document(
                &collection,
                None,
                serde_json::json!({
                    "title": "alpha",
                    "score": 7
                }),
            )
            .unwrap();
    }

    #[test]
    fn should_create_select_drop_user_defined_view() {
        // Arrange
        use_local_storage();
        let path = data_dir("create_select_drop");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let collection = "view_docs";
            seed_view_docs(&cassie, collection);
            let session = cassie.create_session("tester", None);

            // Act
            cassie
                .execute_sql(
                    &session,
                    "CREATE VIEW view_docs_ready AS SELECT title, score FROM view_docs",
                    vec![],
                )
                .unwrap();
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title, score FROM view_docs_ready WHERE score = 7",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(&session, "DROP VIEW view_docs_ready", vec![])
                .unwrap();
            let dropped = cassie.execute_sql(&session, "SELECT title FROM view_docs_ready", vec![]);

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("alpha".to_string()), Value::Int64(7),]]
            );
            assert!(dropped.is_err());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_select_from_nested_user_defined_views() {
        // Arrange
        use_local_storage();
        let path = data_dir("nested");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let collection = "view_nested_docs";
            seed_view_docs(&cassie, collection);
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE VIEW view_nested_inner AS SELECT title FROM view_nested_docs",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE VIEW view_nested_outer AS SELECT title FROM view_nested_inner",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM view_nested_outer WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("alpha".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_hydrate_user_defined_views_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let collection = "view_restart_docs";
            seed_view_docs(&cassie, collection);
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE VIEW view_restart_ready AS SELECT title, score FROM view_restart_docs",
                    vec![],
                )
                .unwrap();
            drop(cassie);

            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            let session = restarted.create_session("tester", None);

            // Act
            let selected = restarted
                .execute_sql(
                    &session,
                    "SELECT title, score FROM view_restart_ready WHERE score = 7",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![Value::String("alpha".to_string()), Value::Int64(7)]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_dml_against_user_defined_view() {
        // Arrange
        use_local_storage();
        let path = data_dir("read_only");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let collection = "view_read_only_docs";
            seed_view_docs(&cassie, collection);
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE VIEW view_read_only AS SELECT title, score FROM view_read_only_docs",
                    vec![],
                )
                .unwrap();

            // Act
            let insert = cassie.execute_sql(
                &session,
                "INSERT INTO view_read_only (title, score) VALUES ('beta', 9)",
                vec![],
            );
            let update =
                cassie.execute_sql(&session, "UPDATE view_read_only SET score = 9", vec![]);
            let delete = cassie.execute_sql(&session, "DELETE FROM view_read_only", vec![]);

            // Assert
            assert!(matches!(insert, Err(error) if error.to_string().contains("read-only")));
            assert!(matches!(update, Err(error) if error.to_string().contains("read-only")));
            assert!(matches!(delete, Err(error) if error.to_string().contains("read-only")));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/catalog_support_contract.rs.
mod catalog_support_contract {
    #[test]
    fn should_publish_the_stable_named_client_catalog_subset() {
        // Arrange
        let feature_support = include_str!("../docs/feature-support.md");
        let catalog_contract = include_str!("../docs/catalog-support.md");
        let readiness = include_str!("../docs/production-readiness.md");
        let evidence = include_str!("../docs/promotion-evidence-matrix.md");

        // Act
        let named_clients = ["sqlx 0.8.3", "Diesel 2.2.6"];
        let stable_views = ["information_schema.tables", "information_schema.columns"];

        // Assert
        for view in stable_views {
            assert!(feature_support.contains(&format!("| `{view}` |")));
            assert!(catalog_contract.contains(&format!("### `{view}`")));
        }
        for client in named_clients {
            assert!(catalog_contract.contains(client));
        }
        assert!(catalog_contract.contains("explicit `ORDER BY`"));
        assert!(catalog_contract.contains("SQLSTATE `42P01`"));
        assert!(catalog_contract.contains("PostgreSQL-internal catalog parity is not claimed"));
        assert!(readiness.contains("stable named-client catalog subset"));
        assert!(evidence.contains("Named-client catalog subset promoted"));
    }
}

// Formerly tests/schema_write_conflicts.rs.
mod schema_write_conflicts {
    use super::support_sql as support;

    use std::sync::{Arc, Barrier};

    use cassie::app::{Cassie, CassieError};
    use cassie::midge::adapter::{
        schema_write_conflict_test_guard, schema_write_conflict_worker_guard,
        set_schema_write_commit_barriers, SchemaWritePausePoint,
    };

    #[test]
    fn should_abort_one_conflicting_table_create_before_reusing_object_id() {
        // Arrange
        support::use_local_storage();
        let _test_guard = schema_write_conflict_test_guard();
        let path = support::data_dir("schema-write-conflicts");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
        cassie.startup().expect("start Cassie");
        let ready = Arc::new(Barrier::new(3));
        let resume = Arc::new(Barrier::new(3));
        set_schema_write_commit_barriers(
            Some(SchemaWritePausePoint::CollectionCreate),
            Some(Arc::clone(&ready)),
            Some(Arc::clone(&resume)),
        );
        let statements = [
            (
                "schema_conflict_alpha",
                "CREATE TABLE schema_conflict_alpha (value TEXT)",
            ),
            (
                "schema_conflict_beta",
                "CREATE TABLE schema_conflict_beta (value TEXT)",
            ),
        ];
        let workers = statements.map(|(table, sql)| {
            let worker_cassie = Arc::clone(&cassie);
            std::thread::spawn(move || {
                let _worker_guard = schema_write_conflict_worker_guard();
                let session = worker_cassie.create_session("tester", None);
                (table, sql, worker_cassie.execute_sql(&session, sql, vec![]))
            })
        });
        ready.wait();
        set_schema_write_commit_barriers(None, None, None);

        // Act
        resume.wait();
        let outcomes = workers.map(|worker| worker.join().expect("join schema worker"));
        let mut successes = 0_usize;
        let mut retryable_conflicts = 0_usize;
        let mut losing_statement = None;
        for (_, sql, result) in outcomes {
            match result {
                Ok(_) => successes += 1,
                Err(CassieError::StorageRetryable(message))
                    if message
                        .to_ascii_lowercase()
                        .starts_with("midge write conflict") =>
                {
                    retryable_conflicts += 1;
                    losing_statement = Some(sql);
                }
                Err(error) => panic!("unexpected schema creation error: {error}"),
            }
        }
        assert_eq!(successes, 1, "exactly one conflicting DDL should commit");
        assert_eq!(
            retryable_conflicts, 1,
            "exactly one conflicting DDL should abort"
        );
        let retry_session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &retry_session,
                losing_statement.expect("one losing schema statement"),
                vec![],
            )
            .expect("retry conflicting schema statement");
        let object_ids = statements.map(|(table, _)| {
            cassie
                .midge
                .collection_metadata(table)
                .expect("read collection metadata")
                .expect("created collection metadata")
                .storage_id
        });

        // Assert
        assert!(object_ids.iter().all(|object_id| *object_id > 0));
        assert_ne!(object_ids[0], object_ids[1]);

        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_abort_one_conflicting_nextval_before_returning_a_duplicate_value() {
        // Arrange
        support::use_local_storage();
        let _test_guard = schema_write_conflict_test_guard();
        let path = support::data_dir("sequence-write-conflicts");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
        cassie.startup().expect("start Cassie");
        let setup_session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &setup_session,
                "CREATE SEQUENCE sequence_conflict_ids",
                vec![],
            )
            .expect("create sequence");
        let ready = Arc::new(Barrier::new(3));
        let resume = Arc::new(Barrier::new(3));
        set_schema_write_commit_barriers(
            Some(SchemaWritePausePoint::SequenceNextValue),
            Some(Arc::clone(&ready)),
            Some(Arc::clone(&resume)),
        );
        let workers = [(), ()].map(|()| {
            let worker_cassie = Arc::clone(&cassie);
            std::thread::spawn(move || {
                let _worker_guard = schema_write_conflict_worker_guard();
                worker_cassie
                    .midge
                    .next_sequence_value("sequence_conflict_ids")
            })
        });
        ready.wait();
        set_schema_write_commit_barriers(None, None, None);

        // Act
        resume.wait();
        let outcomes = workers.map(|worker| worker.join().expect("join sequence worker"));
        let mut returned_ids = Vec::new();
        let mut retryable_conflicts = 0_usize;
        for result in outcomes {
            match result {
                Ok(value) => returned_ids.push(value),
                Err(CassieError::StorageRetryable(message))
                    if message
                        .to_ascii_lowercase()
                        .starts_with("midge write conflict") =>
                {
                    retryable_conflicts += 1;
                }
                Err(error) => panic!("unexpected sequence error: {error}"),
            }
        }
        assert_eq!(returned_ids.len(), 1, "one nextval call should commit");
        assert_eq!(retryable_conflicts, 1, "one nextval call should abort");
        returned_ids.push(
            cassie
                .midge
                .next_sequence_value("sequence_conflict_ids")
                .expect("retry nextval"),
        );

        // Assert
        returned_ids.sort_unstable();
        assert_eq!(returned_ids, vec![1, 2]);
        let stored = cassie
            .midge
            .get_sequence("sequence_conflict_ids")
            .expect("read sequence")
            .expect("stored sequence");
        assert_eq!(stored.current_value, 2);

        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_concurrent_database_creates_across_retry_restart() {
        // Arrange
        support::use_local_storage();
        let _test_guard = schema_write_conflict_test_guard();
        let path = support::data_dir("database-write-conflicts");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
        cassie.startup().expect("start Cassie");
        let ready = Arc::new(Barrier::new(3));
        let resume = Arc::new(Barrier::new(3));
        set_schema_write_commit_barriers(
            Some(SchemaWritePausePoint::DatabaseCreateFinalize),
            Some(Arc::clone(&ready)),
            Some(Arc::clone(&resume)),
        );
        let workers = ["database_conflict_alpha", "database_conflict_beta"].map(|database| {
            let worker_cassie = Arc::clone(&cassie);
            std::thread::spawn(move || {
                let _worker_guard = schema_write_conflict_worker_guard();
                (
                    database,
                    worker_cassie.midge.create_database(database, None),
                )
            })
        });
        ready.wait();
        set_schema_write_commit_barriers(None, None, None);

        // Act
        resume.wait();
        let outcomes = workers.map(|worker| worker.join().expect("join database worker"));
        let mut successes = 0_usize;
        let mut losing_database = None;
        for (database, result) in outcomes {
            match result {
                Ok(()) => successes += 1,
                Err(CassieError::StorageRetryable(message))
                    if message
                        .to_ascii_lowercase()
                        .starts_with("midge write conflict") =>
                {
                    losing_database = Some(database);
                }
                Err(error) => panic!("unexpected database creation error: {error}"),
            }
        }
        assert_eq!(successes, 1, "one database creation should commit");
        cassie
            .midge
            .create_database(losing_database.expect("one losing database creation"), None)
            .expect("retry database creation");
        let names_before_restart = cassie
            .midge
            .list_databases()
            .expect("list databases before restart")
            .into_iter()
            .map(|database| database.name)
            .collect::<Vec<_>>();
        assert!(names_before_restart.contains(&"database_conflict_alpha".to_string()));
        assert!(names_before_restart.contains(&"database_conflict_beta".to_string()));
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("restart Cassie");

        // Assert
        let names_after_restart = restarted
            .midge
            .list_databases()
            .expect("list databases after restart")
            .into_iter()
            .map(|database| database.name)
            .collect::<Vec<_>>();
        assert!(names_after_restart.contains(&"database_conflict_alpha".to_string()));
        assert!(names_after_restart.contains(&"database_conflict_beta".to_string()));
        assert!(restarted.catalog.database_exists("database_conflict_alpha"));
        assert!(restarted.catalog.database_exists("database_conflict_beta"));

        drop(restarted);
        let _ = std::fs::remove_dir_all(path);
    }
}
