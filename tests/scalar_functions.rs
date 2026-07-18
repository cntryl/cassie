use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::sql::ast::{Expr, QueryStatement, SelectItem};
use cassie::sql::parser::parse_statement;
use cassie::types::{DataType, FieldSchema, Schema, Value};
use tokio_postgres::{NoTls, SimpleQueryMessage};
use uuid::Uuid;

fn with_fallback() {
    env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-scalar-functions-{}-{}",
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
}

#[test]
fn should_parse_common_scalar_function_calls() {
    // Arrange
    let sql =
        "SELECT concat(lower(title), coalesce(status, 'unknown'), substring(code, 2, 3)) FROM docs";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let SelectItem::Function { function, .. } = &statement.projection[0] else {
        panic!("expected function projection");
    };
    assert_eq!(function.name, "concat");
    assert_eq!(function.args.len(), 3);
    assert!(matches!(&function.args[0], Expr::Function(inner) if inner.name == "lower"));
    assert!(matches!(&function.args[1], Expr::Function(inner) if inner.name == "coalesce"));
    assert!(matches!(&function.args[2], Expr::Function(inner) if inner.name == "substring"));
}

#[test]
fn should_execute_string_scalar_functions_in_query_path() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("string_helpers");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        let table = "scalar_string_helpers";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(table, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                table,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                table,
                Some("d1".to_string()),
                serde_json::json!({"id": "d1", "title": "  Alpha  "}),
            )

            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT lower(title) AS lowered, upper(title) AS uppered, trim(title) AS trimmed, substring(trim(title), 2, 3) AS sliced, concat(lower(trim(title)), '-', 'suffix') AS combined, length(trim(title)) AS length_value, len(trim(title)) AS len_value FROM scalar_string_helpers WHERE lower(trim(title)) = 'alpha' ORDER BY len(trim(title)) DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.columns.len(), 7);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("  alpha  ".to_string()));
        assert_eq!(result.rows[0][1], Value::String("  ALPHA  ".to_string()));
        assert_eq!(result.rows[0][2], Value::String("Alpha".to_string()));
        assert_eq!(result.rows[0][3], Value::String("lph".to_string()));
        assert_eq!(
            result.rows[0][4],
            Value::String("alpha-suffix".to_string())
        );
        assert_eq!(result.rows[0][5], Value::Int64(5));
        assert_eq!(result.rows[0][6], Value::Int64(5));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_null_numeric_scalar_functions() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("coalesce_abs");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        let table = "scalar_null_helpers";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
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
            .create_collection(table, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                table,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                table,
                Some("d1".to_string()),
                serde_json::json!({"id": "d1", "title": null, "score": -4}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                table,
                Some("d2".to_string()),
                serde_json::json!({"id": "d2", "title": "beta", "score": 9}),
            )

            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, coalesce(title, 'fallback') AS resolved, abs(score) AS absolute_score FROM scalar_null_helpers ORDER BY abs(score) DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(
            result.rows[0],
            vec![
                Value::String("d2".to_string()),
                Value::String("beta".to_string()),
                Value::Int64(9),
            ]
        );
        assert_eq!(
            result.rows[1],
            vec![
                Value::String("d1".to_string()),
                Value::String("fallback".to_string()),
                Value::Int64(4),
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_short_circuit_coalesce_before_evaluating_later_arguments() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("coalesce_short_circuit");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        let table = "scalar_coalesce_short_circuit";

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
            .create_collection(table, schema.clone())
            .unwrap();
        cassie.register_collection(
            table,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        cassie
            .midge
            .put_document(
                table,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "score": 7}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT coalesce(title, lower(score)) FROM scalar_coalesce_short_circuit",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_scalar_function_with_invalid_arity() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("arity_error");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        let table = "scalar_arity_error";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(table, schema.clone())
            .unwrap();
        cassie.register_collection(
            table,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        cassie
            .midge
            .put_document(
                table,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();

        // Act
        let result = cassie.execute_sql(
            &session,
            "SELECT lower(title, title) FROM scalar_arity_error",
            vec![],
        );

        // Assert
        let error = result.expect_err("query should fail");
        assert!(error.to_string().contains("lower"));
        assert!(error.to_string().contains("expects"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_scalar_function_with_unsupported_type() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("type_error");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        let table = "scalar_type_error";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(table, schema.clone())
            .unwrap();
        cassie.register_collection(
            table,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        cassie
            .midge
            .put_document(
                table,
                Some("d1".to_string()),
                serde_json::json!({"score": 7}),
            )
            .unwrap();

        // Act
        let result = cassie.execute_sql(
            &session,
            "SELECT lower(score) FROM scalar_type_error",
            vec![],
        );

        // Assert
        let error = result.expect_err("query should fail");
        assert!(error.to_string().to_lowercase().contains("text"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_scalar_functions_through_pgwire() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("pgwire").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        let messages = tokio::time::timeout(
            Duration::from_secs(5),
            client.simple_query(
                "SELECT lower('ALPHA'), upper('beta'), trim('  gamma  '), substring('delta', 2, 3), concat('a', 'b'), coalesce(NULL, 'fallback'), length('zoo'), len('zoo'), abs(-4)",
            ),
        )
        .await
        .expect("query should complete within the timeout")
        .expect("pgwire query result");

        // Assert
        let row = messages
            .into_iter()
            .find_map(|message| match message {
                SimpleQueryMessage::Row(row) => Some(row),
                _ => None,
            })
            .expect("query should return a row");
        assert_eq!(row.get(0), Some("alpha"));
        assert_eq!(row.get(1), Some("BETA"));
        assert_eq!(row.get(2), Some("gamma"));
        assert_eq!(row.get(3), Some("elt"));
        assert_eq!(row.get(4), Some("ab"));
        assert_eq!(row.get(5), Some("fallback"));
        assert_eq!(row.get(6), Some("3"));
        assert_eq!(row.get(7), Some("3"));
        assert_eq!(row.get(8), Some("4"));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_execute_user_defined_functions_after_builtin_expansion() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("udf_regression");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        let table = "scalar_udf_regression";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(table, schema.clone())
            .unwrap();
        cassie.register_collection(
            table,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        cassie
            .midge
            .put_document(
                table,
                Some("d1".to_string()),
                serde_json::json!({"title": "Alpha"}),
            )
            .unwrap();

        cassie
            .execute_sql(
                &session,
                r#"CREATE FUNCTION echo_text(x TEXT) RETURNS TEXT AS "x""#,
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT echo_text(lower(title)) FROM scalar_udf_regression",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_bucket_timestamps_into_fixed_windows() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("time_bucket_fixed_windows");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE time_bucket_fixed_windows (event_at TIMESTAMP)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO time_bucket_fixed_windows (event_at) VALUES ('2024-01-01T00:00:00Z')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO time_bucket_fixed_windows (event_at) VALUES ('2024-01-01T00:14:59Z')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO time_bucket_fixed_windows (event_at) VALUES ('2024-01-01T00:15:00Z')",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('15 minutes', event_at) AS bucket FROM time_bucket_fixed_windows ORDER BY event_at",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("2024-01-01T00:00:00Z".to_string())],
                vec![Value::String("2024-01-01T00:00:00Z".to_string())],
                vec![Value::String("2024-01-01T00:15:00Z".to_string())],
            ]
        );
        assert_eq!(result.columns[0].data_type, "timestamp");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_bucket_timestamps_with_custom_origin() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("time_bucket_origin");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_time_bucket_origin (id INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_time_bucket_origin (id) VALUES (1)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('1 hour', '1969-12-31T23:30:00Z'), time_bucket('15 minutes', '2024-01-01T00:19:00Z', '2024-01-01T00:05:00Z') FROM scalar_time_bucket_origin",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            result.rows[0],
            vec![
                Value::String("1969-12-31T23:00:00Z".to_string()),
                Value::String("2024-01-01T00:05:00Z".to_string()),
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_validate_time_bucket_nulls_errors() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("time_bucket_errors");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_time_bucket_errors (event_at TIMESTAMP)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_time_bucket_errors (event_at) VALUES (NULL)",
                vec![],
            )
            .unwrap();

        // Act
        let null_result = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('15 minutes', event_at) FROM scalar_time_bucket_errors",
                vec![],
            )
            .unwrap();
        let zero_width = cassie.execute_sql(
            &session,
            "SELECT time_bucket('0 seconds', '2024-01-01T00:00:00Z') FROM scalar_time_bucket_errors",
            vec![],
        );
        let month_width = cassie.execute_sql(
            &session,
            "SELECT time_bucket('1 month', '2024-01-01T00:00:00Z') FROM scalar_time_bucket_errors",
            vec![],
        );
        let bad_type = cassie.execute_sql(
            &session,
            "SELECT time_bucket(15, '2024-01-01T00:00:00Z') FROM scalar_time_bucket_errors",
            vec![],
        );

        // Assert
        assert_eq!(null_result.rows, vec![vec![Value::Null]]);
        assert!(zero_width.unwrap_err().to_string().contains("positive"));
        assert!(month_width.unwrap_err().to_string().contains("calendar"));
        assert!(bad_type.unwrap_err().to_string().contains("duration string"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_time_bucket_grouping_having_ordering() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("time_bucket_grouping");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_time_bucket_grouping (event_at TIMESTAMP)",
                vec![],
            )
            .unwrap();
        for event_at in [
            "2024-01-01T00:01:00Z",
            "2024-01-01T00:14:00Z",
            "2024-01-01T00:16:00Z",
        ] {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO scalar_time_bucket_grouping (event_at) VALUES ('{event_at}')"
                    ),
                    vec![],
                )
                .unwrap();
        }

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('15 minutes', event_at) AS bucket, count(*) FROM scalar_time_bucket_grouping GROUP BY time_bucket('15 minutes', event_at) HAVING count(*) > 1 ORDER BY bucket",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::String("2024-01-01T00:00:00Z".to_string()),
                Value::Int64(2),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_time_bucket_through_pgwire() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("time_bucket_pgwire").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .simple_query("CREATE TABLE scalar_time_bucket_pgwire (event_at TIMESTAMP)")
            .await
            .unwrap();
        client
            .simple_query(
                "INSERT INTO scalar_time_bucket_pgwire (event_at) VALUES ('2024-01-01T00:29:00Z')",
            )
            .await
            .unwrap();
        let messages = client
            .simple_query(
                "SELECT time_bucket('15 minutes', event_at) FROM scalar_time_bucket_pgwire",
            )
            .await
            .unwrap();

        // Assert
        let row = messages
            .iter()
            .find_map(|message| match message {
                SimpleQueryMessage::Row(row) => Some(row),
                _ => None,
            })
            .expect("time_bucket row");
        assert_eq!(row.get(0), Some("2024-01-01T00:15:00Z"));

        drop(client);
        server.shutdown(connection).await;
    });
}
