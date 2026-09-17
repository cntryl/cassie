// Consolidated integration suite: executor.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/executor.rs"]
mod support_executor;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/admission_control.rs.
mod admission_control {
    use super::support_pgwire as pgwire;

    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use pgwire::{complete_startup, parse_error_fields, read_wire_frame};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use uuid::Uuid;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn data_dir(label: &str) -> String {
        let mut path = std::env::temp_dir();
        path.push(format!("cassie-admission-{label}-{}", Uuid::new_v4()));
        path.to_string_lossy().to_string()
    }

    fn runtime_config_with_limits(pgwire_max: usize, rest_max: usize) -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        config.limits.pgwire_max_connections = pgwire_max;
        config.limits.rest_max_connections = rest_max;
        config
    }

    fn tls_identity(label: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("cassie-admission-tls-{label}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create TLS directory");
        let certificate = directory.join("cert.pem");
        let key = directory.join("key.pem");
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("certificate identity");
        std::fs::write(&certificate, identity.cert.pem()).expect("certificate fixture");
        std::fs::write(&key, identity.signing_key.serialize_pem()).expect("key fixture");
        (directory, certificate, key)
    }

    fn error_field(payload: &[u8], field: char) -> Option<String> {
        parse_error_fields(payload)
            .into_iter()
            .find_map(|(tag, value)| (tag == field).then_some(value))
    }

    async fn read_http_response_head(stream: &mut tokio::net::TcpStream) -> String {
        let mut response = Vec::new();
        let mut buf = [0u8; 1];
        while !response.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut buf).await.expect("read http byte");
            response.push(buf[0]);
        }
        String::from_utf8(response).expect("http response should be utf-8")
    }

    #[test]
    fn should_reject_pgwire_connections_over_admission_limit() {
        // Arrange
        use_local_storage();
        let path = data_dir("pgwire");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = runtime_config_with_limits(1, 512);
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(cassie::pgwire::server::run(
                addr.to_string(),
                std::sync::Arc::new(cassie),
                config,
            ));
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let mut held = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect held pgwire");
            {
                let (mut held_reader, mut held_writer) = held.split();
                complete_startup(&mut held_reader, &mut held_writer).await;
            }

            // Act
            let mut overflow = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect overflow pgwire");
            let (tag, payload) = read_wire_frame(&mut overflow).await;

            // Assert
            assert_eq!(tag, b'E');
            assert_eq!(error_field(&payload, 'C').as_deref(), Some("53300"));
            assert_eq!(error_field(&payload, 'S').as_deref(), Some("FATAL"));

            drop(held);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let mut later = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect later pgwire");
            let (mut later_reader, mut later_writer) = later.split();
            complete_startup(&mut later_reader, &mut later_writer).await;

            server.abort();
            let _ = server.await;
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_release_rest_admission_permit_after_overflow_503() {
        // Arrange
        use_local_storage();
        let path = data_dir("rest");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let config = runtime_config_with_limits(256, 1);
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie.clone()));
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let mut held = tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect held rest");
            held.write_all(
                b"GET /health HTTP/1.1\r\nhost: localhost\r\nconnection: keep-alive\r\n\r\n",
            )
            .await
            .expect("write held request");
            let held_head = read_http_response_head(&mut held).await;
            assert!(held_head.starts_with("HTTP/1.1 200"), "{held_head}");

            let client = reqwest::Client::new();

            // Act
            let overflow = client
                .get(format!("http://{addr}/health"))
                .send()
                .await
                .expect("overflow request");
            let overflow_status = overflow.status();
            let overflow_connection = overflow
                .headers()
                .get(reqwest::header::CONNECTION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let overflow_body = overflow.text().await.expect("overflow body");

            // Assert
            assert_eq!(overflow_status.as_u16(), 503);
            assert_eq!(overflow_connection.as_deref(), Some("close"));
            assert!(
                overflow_body.contains("too many connections"),
                "body={overflow_body}"
            );

            drop(held);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let later = client
                .get(format!("http://{addr}/health"))
                .send()
                .await
                .expect("later request");
            assert!(later.status().is_success());

            server.abort();
            let _ = server.await;
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_emit_no_plaintext_pgwire_rejection_before_required_tls() {
        // Arrange
        use_local_storage();
        let path = data_dir("pgwire-tls-overflow");
        let (tls_dir, certificate, key) = tls_identity("pgwire-overflow");
        let mut config = runtime_config_with_limits(1, 512);
        config.password = "non-default-secret".to_string();
        config.pgwire_tls_cert_file = Some(certificate.to_string_lossy().to_string());
        config.pgwire_tls_key_file = Some(key.to_string_lossy().to_string());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
                .await
                .expect("reserve listener");
            let port = listener.local_addr().expect("listener address").port();
            drop(listener);
            let server = tokio::spawn(cassie::pgwire::server::run(
                format!("0.0.0.0:{port}"),
                std::sync::Arc::new(cassie),
                config,
            ));
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
            let held = tokio::net::TcpStream::connect(address)
                .await
                .expect("connect held pgwire");

            // Act
            let mut overflow = tokio::net::TcpStream::connect(address)
                .await
                .expect("connect overflow pgwire");
            let mut byte = [0_u8; 1];
            let observed = tokio::time::timeout(
                std::time::Duration::from_millis(250),
                overflow.read(&mut byte),
            )
            .await;

            // Assert
            assert!(
                matches!(observed, Ok(Ok(0) | Err(_))),
                "TLS-required admission rejection exposed plaintext: {observed:?}"
            );
            drop(held);
            server.abort();
            let _ = server.await;
        });

        let _ = std::fs::remove_dir_all(path);
        let _ = std::fs::remove_dir_all(tls_dir);
    }

    #[test]
    fn should_return_rest_admission_503_over_tls() {
        // Arrange
        use_local_storage();
        let path = data_dir("rest-tls-overflow");
        let (tls_dir, certificate, key) = tls_identity("rest-overflow");
        let mut config = runtime_config_with_limits(256, 1);
        config.rest_tls_cert_file = Some(certificate.to_string_lossy().to_string());
        config.rest_tls_key_file = Some(key.to_string_lossy().to_string());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("reserve listener");
            let addr = listener.local_addr().expect("listener address");
            drop(listener);
            let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie));
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let held_client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .expect("held TLS client");
            let held = held_client
                .get(format!("https://{addr}/health"))
                .send()
                .await
                .expect("held TLS request");
            assert!(held.status().is_success());
            let overflow_client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .expect("overflow TLS client");

            // Act
            let overflow = overflow_client
                .get(format!("https://{addr}/health"))
                .send()
                .await;

            // Assert
            let overflow = overflow.expect("TLS admission rejection response");
            assert_eq!(overflow.status().as_u16(), 503);
            assert!(overflow
                .text()
                .await
                .expect("overflow body")
                .contains("too many connections"));
            drop(held);
            server.abort();
            let _ = server.await;
        });

        let _ = std::fs::remove_dir_all(path);
        let _ = std::fs::remove_dir_all(tls_dir);
    }
}

// Formerly tests/application_pipeline_schema.rs.
mod application_pipeline_schema {
    use super::support_sql as support;
    use cassie::app::Cassie;
    use support::*;

    const PIPELINE_APPLICATION_STATEMENTS: &[&str] = &[
        r#"CREATE TABLE "pipeline_statuses" (
  "id" UUID NOT NULL DEFAULT gen_random_uuid(),
  "entity_kind" TEXT NOT NULL,
  "entity_id" TEXT NOT NULL,
  "entity_type" TEXT NOT NULL,
  "current_state" TEXT NOT NULL,
  "previous_state" TEXT,
  "status_updated_at" TIMESTAMP(3),
  "project" JSONB,
  "selected_partner" JSONB,
  "partner_eligibility" JSONB,
  "company_name" TEXT,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated_at" TIMESTAMP(3) NOT NULL,
  CONSTRAINT "pipeline_statuses_pkey" PRIMARY KEY ("id")
)"#,
        r#"CREATE TABLE "pipeline_status_history" (
  "id" UUID NOT NULL DEFAULT gen_random_uuid(),
  "entity_kind" TEXT NOT NULL,
  "entity_id" TEXT NOT NULL,
  "event_id" TEXT,
  "state" TEXT NOT NULL,
  "previous_state" TEXT,
  "action" TEXT,
  "source" TEXT,
  "entered_at" TIMESTAMP(3),
  "sequence" INTEGER,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "status_id" UUID,
  CONSTRAINT "pipeline_status_history_pkey" PRIMARY KEY ("id")
)"#,
        r#"CREATE TABLE "pipeline_state_progression" (
  "id" UUID NOT NULL DEFAULT gen_random_uuid(),
  "group" TEXT NOT NULL,
  "state" TEXT NOT NULL,
  "label" TEXT,
  "sort_order" INTEGER NOT NULL,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated_at" TIMESTAMP(3) NOT NULL,
  CONSTRAINT "pipeline_state_progression_pkey" PRIMARY KEY ("id")
)"#,
        r#"CREATE TABLE "microf_brands" (
  "id" TEXT NOT NULL,
  "name" TEXT NOT NULL,
  "active" BOOLEAN NOT NULL DEFAULT true,
  "sort_order" INTEGER NOT NULL DEFAULT 0,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated_at" TIMESTAMP(3) NOT NULL,
  CONSTRAINT "microf_brands_pkey" PRIMARY KEY ("id")
)"#,
        r#"CREATE TABLE "scheduled_charges" (
  "id" UUID NOT NULL DEFAULT gen_random_uuid(),
  "scheduled_charge_id" TEXT NOT NULL,
  "application_id" TEXT NOT NULL,
  "status" TEXT NOT NULL,
  "due_at" TIMESTAMP(3),
  "properties" JSONB,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated_at" TIMESTAMP(3) NOT NULL,
  CONSTRAINT "scheduled_charges_pkey" PRIMARY KEY ("id")
)"#,
        r#"CREATE TABLE "partner_offers" (
  "id" UUID NOT NULL DEFAULT gen_random_uuid(),
  "application_id" TEXT NOT NULL,
  "partner" TEXT NOT NULL,
  "partner_type" TEXT,
  "offer_id" TEXT,
  "status" TEXT NOT NULL,
  "details" JSONB,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated_at" TIMESTAMP(3) NOT NULL,
  CONSTRAINT "partner_offers_pkey" PRIMARY KEY ("id")
)"#,
        r#"CREATE UNIQUE INDEX "pipeline_statuses_entity_key" ON "pipeline_statuses"("entity_kind", "entity_id")"#,
        r#"CREATE UNIQUE INDEX "pipeline_status_history_event_id_key" ON "pipeline_status_history"("event_id")"#,
        r#"CREATE INDEX "pipeline_status_history_entity_sequence_idx" ON "pipeline_status_history"("entity_kind", "entity_id", "sequence")"#,
        r#"CREATE UNIQUE INDEX "pipeline_state_progression_group_state_key" ON "pipeline_state_progression"("group", "state")"#,
        r#"CREATE UNIQUE INDEX "scheduled_charges_scheduled_charge_id_key" ON "scheduled_charges"("scheduled_charge_id")"#,
        r#"CREATE INDEX "scheduled_charges_status_due_at_idx" ON "scheduled_charges"("status", "due_at")"#,
        r#"CREATE INDEX "scheduled_charges_application_id_idx" ON "scheduled_charges"("application_id")"#,
        r#"CREATE INDEX "partner_offers_application_partner_type_idx" ON "partner_offers"("application_id", "partner_type")"#,
        r#"ALTER TABLE "pipeline_status_history"
  ADD CONSTRAINT "pipeline_status_history_status_id_fkey"
  FOREIGN KEY ("status_id") REFERENCES "pipeline_statuses"("id")
  ON DELETE CASCADE ON UPDATE CASCADE"#,
    ];

    #[test]
    fn should_apply_pipeline_application_schema() {
        // Arrange
        use_local_storage();
        let path = data_dir("pipeline");
        let path_for_cleanup = path.clone();
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("schema-app", None);

        // Act
        for statement in PIPELINE_APPLICATION_STATEMENTS {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| panic!("failed to apply statement:\n{statement}\n{error}"));
        }

        // Assert
        assert!(cassie.catalog.exists("pipeline_statuses"));
        assert!(cassie.catalog.exists("pipeline_status_history"));
        assert!(cassie
            .catalog
            .get_index("scheduled_charges", "scheduled_charges_status_due_at_idx")
            .is_some());
        assert!(cassie
            .catalog
            .get_constraints("pipeline_status_history")
            .iter()
            .any(
                |constraint| constraint.references_table.as_deref() == Some("pipeline_statuses")
                    && constraint.references_field.as_deref() == Some("id")
            ));

        let _ = std::fs::remove_dir_all(path_for_cleanup);
    }
}

// Formerly tests/executor_commands.rs.
mod executor_commands {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
    use cassie::executor;
    use cassie::planner::logical::LogicalPlan;
    use cassie::planner::physical::{
        AdaptivePlanDiagnostics, OperatorFeedbackPlanDiagnostics, PhysicalAggregatePlan,
        PhysicalJoinPlan, PhysicalPlan, PhysicalTopKPlan, PlanEstimates,
    };
    use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
    use cassie::sql::binder;
    use cassie::sql::parser;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    use super::support_executor as support;
    use support::*;

    #[test]
    fn should_support_projection_aliases_for_function_columns() {
        // Arrange
        use_local_storage();
        let path = data_dir("function_alias");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "exec_function_alias";

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

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "lorem world"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'world') AS score FROM exec_function_alias WHERE title = 'alpha'",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.columns[0].name, "score");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 1);
        match &result.rows[0][0] {
            cassie::types::Value::Float64(score) => assert!(*score > 0.0),
            _ => panic!("expected float score"),
        }

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_project_function_columns() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_projection_mix";

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

        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "hello world"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "other text"}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, search_score(body, 'world') AS score FROM exec_projection_mix WHERE body LIKE '%world%' ORDER BY id ASC",
                vec![],
            )

.expect("query should execute");

        // Assert
        assert_eq!(result.columns.len(), 2);
        assert_eq!(result.columns[0].name, "title");
        assert_eq!(result.columns[1].name, "score");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 2);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
        match &result.rows[0][1] {
            Value::Float64(score) => assert!(*score > 0.0),
            _ => panic!("expected float score"),
        }
    });
    }

    #[test]
    fn should_fail_unknown_function_during_execution() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_unknown_function";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            let logical = LogicalPlan {
                command: None,
                source: QuerySource::Collection(collection.to_string()),
                collection: collection.to_string(),
                ctes: vec![],
                distinct: false,
                distinct_on: Vec::new(),
                projection: vec![SelectItem::Function {
                    function: FunctionCall {
                        name: "unknown_fn".to_string(),
                        args: vec![Expr::Column("title".to_string())],
                    },
                    alias: Some("score".to_string()),
                }],
                filter: None,
                group_by: vec![],
                having: None,
                order: vec![],
                limit: Some(10),
                offset: Some(0),
                set: None,
            };

            let physical = PhysicalPlan {
                collection: logical.collection.clone(),
                operators: vec![cassie::planner::physical::Operator::Project],
                estimates: PlanEstimates::default(),
                operator_feedback: OperatorFeedbackPlanDiagnostics::default(),
                adaptive_plan: AdaptivePlanDiagnostics::default(),
                read: cassie::planner::physical::PhysicalReadPlan {
                    access_path: cassie::planner::physical::ReadAccessPath::CollectionScan,
                    access_path_reason: "command-path".to_string(),
                    fallback_reason: Some("command".to_string()),
                    ..Default::default()
                },
                top_k: PhysicalTopKPlan::default(),
                join: PhysicalJoinPlan::default(),
                aggregate: PhysicalAggregatePlan::default(),
                projection: cassie::planner::physical::PhysicalProjectionPlan {
                    shape: cassie::planner::physical::ProjectionShape::Other,
                },
                collection_schema: None,
                logical,
            };

            // Act
            let result = executor::run(&cassie, physical, vec![]);

            // Assert
            assert!(result.is_err());
        });
    }

    #[test]
    fn should_execute_table_lifecycle_commands() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Arrange
            use_local_storage();
            let path = data_dir("ddl_command");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            let table_name = "ddl_table";

            // Act
            let create = cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE ddl_table (id TEXT, title TEXT)",
                    vec![],
                )
                .unwrap();
            assert_eq!(create.command, "CREATE TABLE");
            assert_eq!(create.columns.len(), 0);
            assert!(cassie.catalog.exists(table_name));

            cassie
                .midge
                .put_document(
                    table_name,
                    Some("d1".to_string()),
                    serde_json::json!({"id": "d1", "title": "alpha"}),
                )
                .unwrap();

            let alter_add = cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE ddl_table ADD COLUMN status TEXT",
                    vec![],
                )
                .unwrap();
            let alter_rename = cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE ddl_table RENAME TO ddl_table_archive",
                    vec![],
                )
                .unwrap();
            let rename_rows = cassie
                .execute_sql(
                    &session,
                    "SELECT id, status FROM ddl_table_archive ORDER BY id",
                    vec![],
                )
                .unwrap();
            let drop = cassie
                .execute_sql(&session, "DROP TABLE ddl_table_archive", vec![])
                .unwrap();

            // Assert
            assert_eq!(alter_add.command, "ALTER TABLE");
            assert_eq!(alter_rename.command, "ALTER TABLE");
            assert!(!cassie.catalog.exists(table_name));
            assert_eq!(rename_rows.columns.len(), 2);
            assert_eq!(rename_rows.rows.len(), 1);
            assert_eq!(rename_rows.rows[0][0], Value::String("d1".to_string()));
            assert_eq!(drop.command, "DROP TABLE");
            assert!(!cassie.catalog.exists("ddl_table_archive"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_alter_table_rename_column_command() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Arrange
            use_local_storage();
            let path = data_dir("ddl_rename_column_command");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

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
                    "rename_column_docs",
                    Some("d1".to_string()),
                    serde_json::json!({"id": "d1", "title": "alpha"}),
                )
                .unwrap();

            // Act
            let rename = cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE rename_column_docs RENAME COLUMN title TO headline",
                    vec![],
                )
                .unwrap();
            let rows = cassie
                .execute_sql(
                    &session,
                    "SELECT id, headline FROM rename_column_docs ORDER BY id",
                    vec![],
                )
                .unwrap();

            let schema = cassie
                .catalog
                .get_schema("rename_column_docs")
                .expect("schema should exist");

            // Assert
            assert_eq!(rename.command, "ALTER TABLE");
            assert_eq!(rows.rows.len(), 1);
            assert_eq!(rows.rows[0][0], Value::String("d1".to_string()));
            assert_eq!(rows.rows[0][1], Value::String("alpha".to_string()));
            assert!(schema.fields.iter().any(|field| field.name == "headline"));
            assert!(!schema.fields.iter().any(|field| field.name == "title"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_index_lifecycle_commands() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Arrange
            use_local_storage();
            let path = data_dir("ddl_index_command");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE idx_commands (id TEXT, title TEXT)",
                    vec![],
                )
                .unwrap();

            let create_index = cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX idx_title ON idx_commands USING btree (title)",
                    vec![],
                )
                .unwrap();

            let catalog_index = cassie
                .catalog
                .get_index("idx_commands", "idx_title")
                .expect("index should be in catalog");
            let stored_index = cassie
                .midge
                .get_index("idx_commands", "idx_title")
                .unwrap()
                .expect("index should be persisted");

            let drop_index = cassie
                .execute_sql(
                    &session,
                    "DROP INDEX IF EXISTS idx_title ON idx_commands",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(create_index.command, "CREATE INDEX");
            assert_eq!(create_index.columns.len(), 0);
            assert!(!catalog_index.unique);
            assert_eq!(catalog_index.field, "title");
            assert_eq!(stored_index.field, "title");
            assert_eq!(drop_index.command, "DROP INDEX");
            assert!(cassie
                .catalog
                .get_index("idx_commands", "idx_title")
                .is_none());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_create_composite_index_command() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Arrange
            use_local_storage();
            let path = data_dir("ddl_composite_index_command");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE composite_index_docs (id TEXT, title TEXT, score INT)",
                    vec![],
                )
                .unwrap();

            // Act
            let create_index = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_title_score ON composite_index_docs USING btree (title, score)",
                vec![],
            )
            .unwrap();

            let catalog_index = cassie
                .catalog
                .get_index("composite_index_docs", "idx_title_score")
                .expect("index should be in catalog");
            let stored_index = cassie
                .midge
                .get_index("composite_index_docs", "idx_title_score")
                .unwrap()
                .expect("index should be persisted");

            // Assert
            assert_eq!(create_index.command, "CREATE INDEX");
            assert_eq!(create_index.columns.len(), 0);
            assert_eq!(
                catalog_index.fields,
                vec!["title".to_string(), "score".to_string()]
            );
            assert_eq!(
                stored_index.fields,
                vec!["title".to_string(), "score".to_string()]
            );
            assert_eq!(catalog_index.field, "title");
            assert_eq!(stored_index.field, "title");

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_evaluate_user_function_body_after_create() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Arrange
            use_local_storage();
            let path = data_dir("create_function_exec");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            let collection = "udf_eval";

            cassie
                .execute_sql(&session, "CREATE TABLE udf_eval (id TEXT, x INT)", vec![])
                .unwrap();
            cassie.register_collection(
                collection,
                vec![
                    ("id".to_string(), DataType::Text),
                    ("x".to_string(), DataType::Int),
                ]
                .into_iter()
                .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"id": "d1", "x": 3}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d2".to_string()),
                    serde_json::json!({"id": "d2", "x": 7}),
                )
                .unwrap();

            cassie
                .execute_sql(
                    &session,
                    "CREATE FUNCTION double_input(x INT) RETURNS INT AS \"x\"",
                    vec![],
                )
                .unwrap();

            // Act
            let query = cassie
                .execute_sql(
                    &session,
                    "SELECT id, double_input(x) AS doubled FROM udf_eval ORDER BY id ASC",
                    vec![],
                )
                .unwrap();

            // Assert
            let function = cassie
                .catalog
                .get_function("double_input")
                .expect("function should be registered");
            assert_eq!(function.name, "double_input");
            assert_eq!(query.columns[1].name, "doubled");
            assert_eq!(
                query.rows[0],
                vec![Value::String("d1".to_string()), Value::Int64(3),]
            );
            assert_eq!(
                query.rows[1],
                vec![Value::String("d2".to_string()), Value::Int64(7),]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_function_use_after_drop() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Arrange
            use_local_storage();
            let path = data_dir("drop_function_exec");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);

            let collection = "udf_drop";

            cassie
                .execute_sql(&session, "CREATE TABLE udf_drop (id TEXT, x INT)", vec![])
                .unwrap();
            cassie.register_collection(
                collection,
                vec![
                    ("id".to_string(), DataType::Text),
                    ("x".to_string(), DataType::Int),
                ]
                .into_iter()
                .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"id": "d1", "x": 3}),
                )
                .unwrap();

            cassie
                .execute_sql(
                    &session,
                    "CREATE FUNCTION square(x INT) RETURNS INT AS \"x\"",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(&session, "DROP FUNCTION square", vec![])
                .unwrap();

            // Act
            let result = cassie.execute_sql(&session, "SELECT square(x) FROM udf_drop", vec![]);
            let missing = cassie.catalog.get_function("square").is_none();

            // Assert
            assert!(missing);
            assert!(result.is_err());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_procedure_body_with_arguments_after_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("procedure_exec");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(&session, "CREATE TABLE procedure_exec (title TEXT)", vec![])

.unwrap();
        cassie
            .execute_sql(
                &session,
                r#"CREATE PROCEDURE store_title(title TEXT) AS "INSERT INTO procedure_exec (title) VALUES ($1)""#,
                vec![],
            )
            .unwrap();

        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);

        // Act
        let call = restarted
            .execute_sql(&session, "CALL store_title('alpha')", vec![])

.unwrap();
        let rows = restarted
            .execute_sql(
                &session,
                "SELECT title FROM procedure_exec ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(call.command, "CALL");
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.rows[0][0], Value::String("alpha".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_procedure_bodies_with_transaction_control() {
        // Arrange
        use_local_storage();
        let path = data_dir("procedure_transaction_control");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie.execute_sql(
                &session,
                r#"CREATE PROCEDURE stop_here() AS "BEGIN""#,
                vec![],
            );

            // Assert
            let error = result.expect_err("procedure creation should fail");
            assert!(error
                .to_string()
                .contains("transaction control statements inside procedures"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_recursive_procedure_calls() {
        // Arrange
        use_local_storage();
        let path = data_dir("procedure_recursion");
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
                    r#"CREATE PROCEDURE loop_a() AS "CALL loop_b()""#,
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    r#"CREATE PROCEDURE loop_b() AS "CALL loop_a()""#,
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie.execute_sql(&session, "CALL loop_a()", vec![]);
            // Assert
            let error = result.expect_err("recursive call should fail");
            assert!(error.to_string().contains("recursively invoked"));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/executor_fulltext_scoring.rs.
mod executor_fulltext_scoring {
    use super::support_executor as support;

    use cassie::types::Value;

    use support::{cassie_temp, create_text_collection, put_document, put_fulltext_index};

    fn assert_f64_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= f64::EPSILON,
            "expected {actual} to equal {expected}"
        );
    }

    #[test]
    fn should_apply_fulltext_index_params_during_search_score() {
        // Arrange
        let cassie = cassie_temp("fulltext_k1_b");
        let collection = "exec_fulltext_k1_b";
        // No declared `id` field: `WHERE id = 'd1'` below resolves to the
        // document's internal identity, not a real schema column.
        create_text_collection(&cassie, collection, &["body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "alpha alpha alpha"}),
        );
        put_document(
            &cassie,
            collection,
            "d2",
            serde_json::json!({"body": "bravo"}),
        );

        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_exec_fulltext_k1_b ON exec_fulltext_k1_b USING fulltext (body) WITH (k1 = 0, b = 0)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_k1_b WHERE id = 'd1'",
                vec![],
            )
            .expect("query should execute");

        // Assert
        let expected = cassie::search::bm25::bm25_score(3.0, 1.0, 2.0, 0.0, 0.0, 3.0, 2.0);
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.columns[0].name, "score");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 1);
        match &result.rows[0][0] {
            Value::Float64(score) => assert_f64_close(*score, expected),
            _ => panic!("expected float score"),
        }
    }

    #[test]
    fn should_apply_fulltext_analyzer_stop_words_during_search_score() {
        // Arrange
        let cassie = cassie_temp("fulltext_analyzer_stop_words");
        let collection = "exec_fulltext_analyzer_stop_words";
        create_text_collection(&cassie, collection, &["id", "body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "the the alpha"}),
        );

        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_exec_fulltext_analyzer_stop_words ON exec_fulltext_analyzer_stop_words USING fulltext (body) WITH (analyzer = standard, stop_words = none)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'the') AS score FROM exec_fulltext_analyzer_stop_words",
                vec![],
            )
            .expect("query should execute");

        // Assert
        match &result.rows[0][0] {
            Value::Float64(score) => assert!(*score > 0.0),
            _ => panic!("expected float score"),
        }
    }

    #[test]
    fn should_keep_default_fulltext_case_folding_during_search_score() {
        // Arrange
        let cassie = cassie_temp("fulltext_default_case_folding");
        let collection = "exec_fulltext_default_case_folding";
        // No declared `id` field: `WHERE id = 'd1'` below resolves to the
        // document's internal identity, not a real schema column.
        create_text_collection(&cassie, collection, &["body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "The Rust compiler is fast"}),
        );
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_exec_fulltext_default_case ON exec_fulltext_default_case_folding USING fulltext (body)",
                vec![],
            )
            .expect("create default fulltext index");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'rust') AS score FROM exec_fulltext_default_case_folding WHERE id = 'd1'",
                vec![],
            )
            .expect("score case-folded term");

        // Assert
        assert!(matches!(result.rows[0][0], Value::Float64(score) if score > 0.0));
    }

    #[test]
    fn should_reject_non_finite_fulltext_index_options_during_search_score() {
        // Arrange
        let cassie = cassie_temp("fulltext_non_finite");
        let collection = "exec_fulltext_non_finite";
        create_text_collection(&cassie, collection, &["id", "body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "alpha alpha alpha"}),
        );
        put_fulltext_index(
            &cassie,
            collection,
            "idx_exec_fulltext_non_finite",
            "body",
            &[("boost", "1.0"), ("k1", "1e999"), ("b", "0.75")],
        );
        cassie.hydrate_catalog().unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie.execute_sql(
            &session,
            "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_non_finite WHERE id = 'd1'",
            vec![],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_reject_duplicate_fulltext_indexes_during_search_score() {
        // Arrange
        let cassie = cassie_temp("fulltext_duplicate");
        let collection = "exec_fulltext_duplicate";
        create_text_collection(&cassie, collection, &["id", "body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "alpha alpha alpha"}),
        );
        put_fulltext_index(
            &cassie,
            collection,
            "idx_exec_fulltext_duplicate_a",
            "body",
            &[("boost", "1.0"), ("k1", "1.2"), ("b", "0.75")],
        );
        put_fulltext_index(
            &cassie,
            collection,
            "idx_exec_fulltext_duplicate_b",
            "body",
            &[("boost", "2.0"), ("k1", "0.5"), ("b", "0.4")],
        );
        cassie.hydrate_catalog().unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie.execute_sql(
            &session,
            "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_duplicate WHERE id = 'd1'",
            vec![],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_allow_plain_select_with_non_finite_fulltext_metadata() {
        // Arrange
        let cassie = cassie_temp("plain_select_bad_fulltext");
        let collection = "exec_plain_select_bad_fulltext";
        // No declared `id` field: `WHERE id = 'd1'` below resolves to the
        // document's internal identity, not a real schema column.
        create_text_collection(&cassie, collection, &["body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"body": "alpha beta"}),
        );
        put_fulltext_index(
            &cassie,
            collection,
            "idx_exec_plain_select_bad_fulltext",
            "body",
            &[("boost", "1.0"), ("k1", "inf"), ("b", "0.75")],
        );
        cassie.hydrate_catalog().unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_plain_select_bad_fulltext WHERE id = 'd1'",
                vec![],
            )
            .expect("plain select should execute");

        // Assert
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
    }

    #[test]
    fn should_project_snippet_function_output_for_text_matches() {
        // Arrange
        let cassie = cassie_temp("snippet_output");
        let collection = "exec_snippet_output";
        create_text_collection(&cassie, collection, &["title", "body"]);
        put_document(
            &cassie,
            collection,
            "d1",
            serde_json::json!({"title": "alpha", "body": "Rust enables fast query search"}),
        );

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT snippet(body, 'query') AS excerpt FROM exec_snippet_output WHERE title = 'alpha'",
                vec![],
            )
            .expect("snippet query should execute");

        // Assert
        assert_eq!(result.columns[0].name, "excerpt");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 1);
        match &result.rows[0][0] {
            Value::String(excerpt) => {
                assert_eq!(excerpt, "Rust enables fast <mark>query</mark> search");
            }
            _ => panic!("expected string snippet output"),
        }
    }
}
// Formerly tests/executor_hybrid_scoring.rs.
mod executor_hybrid_scoring {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
    use cassie::executor;
    use cassie::planner::logical::LogicalPlan;
    use cassie::planner::physical::PhysicalPlan;
    use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
    use cassie::sql::binder;
    use cassie::sql::parser;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    use super::support_executor as support;
    use support::*;

    #[test]
    fn should_order_by_hybrid_score() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_hybrid_order";

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
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

        cassie
            .midge
            .put_document(
                collection,
                Some("zeta".to_string()),
                serde_json::json!({"title": "doc1", "body": "red", "embedding": [10.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("alpha".to_string()),
                serde_json::json!({"title": "doc2", "body": "red", "embedding": [1.0, 0.0]}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM exec_hybrid_order ORDER BY score DESC",
                vec![],
            )

.expect("query should execute");

        // Assert
        assert_eq!(result.columns[0].name, "id");
        assert_eq!(result.columns[1].name, "score");
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
        assert_eq!(result.rows[1][0], Value::String("zeta".to_string()));

        let first_score = match &result.rows[0][1] {
            Value::Float64(value) => *value,
            _ => panic!("expected float score"),
        };
        let second_score = match &result.rows[1][1] {
            Value::Float64(value) => *value,
            _ => panic!("expected float score"),
        };
        assert!(first_score > second_score);
    });
    }

    #[test]
    fn should_filter_by_hybrid_score_threshold() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_hybrid_filter";

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
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "doc1", "body": "red apple", "embedding": [1.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "doc2", "body": "green apple", "embedding": [0.0, 2.0]}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_hybrid_filter WHERE hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) > 0.5",
                vec![],
            )

.expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
    });
    }

    #[test]
    fn should_reject_hybrid_score_with_wrong_arity() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_hybrid_wrong_arity";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie.execute_sql(
                &session,
                "SELECT hybrid_score(search_score(body, 'red')) FROM exec_hybrid_wrong_arity",
                vec![],
            );

            // Assert
            let error = result.expect_err("query should reject wrong arity");
            assert!(error.to_string().contains("hybrid_score"));
        });
    }
}

// Formerly tests/executor_limits.rs.
mod executor_limits {
    #![allow(unused_imports, dead_code)]
    use cassie::app::{Cassie, CassieError};
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
    use cassie::executor;
    use cassie::planner::logical::LogicalPlan;
    use cassie::planner::physical::PhysicalPlan;
    use cassie::runtime::QueryExecutionControls;
    use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
    use cassie::sql::binder;
    use cassie::sql::parser;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    use super::support_executor as support;
    use support::*;

    #[test]
    fn should_create_no_deadline_given_a_zero_query_timeout() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Arrange
            use_local_storage();
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.query_timeout_ms = 0;
            let path = data_dir("timeout");
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

            let collection = "exec_timeout";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
                .execute_sql(&session, "SELECT title FROM exec_timeout", vec![])
                .expect("zero query timeout should disable the deadline");

            // Assert
            assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_return_no_partial_rows_given_a_result_limit_overflow() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Arrange
            use_local_storage();
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.max_result_rows = 1;
            let path = data_dir("max_rows");
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

            let collection = "exec_max_rows";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-2".to_string()),
                    serde_json::json!({"title": "beta"}),
                )
                .unwrap();

            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie.execute_sql(
                &session,
                "SELECT title FROM exec_max_rows ORDER BY title",
                vec![],
            );

            // Assert
            let message = result
                .expect_err("query should fail when row limit is configured too low")
                .to_string();
            assert!(
                message.contains("query result row limit exceeded"),
                "expected row limit error, got {message}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_fail_query_when_cte_recursion_depth_is_exceeded() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
    // Arrange
    use_local_storage();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.cte_recursion_depth = 0;
    let path = data_dir("cte_depth");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

    let collection = "exec_cte_depth";
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "n".to_string(),
            data_type: DataType::Int,
            nullable: true,
        }],
    };

    cassie
        .midge
        .create_collection(collection, schema.clone())
        .unwrap();
    cassie.register_collection(
        collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
    cassie
        .midge
        .put_document(
            collection,
            Some("d1".to_string()),
            serde_json::json!({"n": 1}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
            .execute_sql(
                &session,
                "WITH RECURSIVE seq(n) AS (SELECT n FROM exec_cte_depth WHERE n = 1 UNION ALL SELECT n FROM seq WHERE n = 1) SELECT n FROM seq",
                vec![],
            )
            ;

    // Assert
    let message = result
        .expect_err("recursive cte should fail when depth is exhausted")
        .to_string();
    assert!(
        message.contains("exceeded the maximum recursion depth of 0 iterations"),
        "expected recursion depth error, got {message}"
    );

    let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fail_query_with_resource_limit_when_memory_budget_is_exceeded() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Arrange
            use_local_storage();
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.query_memory_budget_bytes = 16;
            let path = data_dir("spill_budget");
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

            let collection = "exec_spill";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "payload".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"payload": "very long payload data for memory budget test"}),
                )
                .unwrap();

            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie.execute_sql(&session, "SELECT payload FROM exec_spill", vec![]);

            // Assert
            let error = result.expect_err("query should fail when memory budget is exhausted");
            assert!(matches!(error, CassieError::ResourceLimit(_)));
            assert!(error.to_string().contains("query memory budget exceeded"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_skip_offset_then_take_limit() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_offset_limit";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "a"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d2".to_string()),
                    serde_json::json!({"title": "b"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d3".to_string()),
                    serde_json::json!({"title": "c"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d4".to_string()),
                    serde_json::json!({"title": "d"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d5".to_string()),
                    serde_json::json!({"title": "e"}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id FROM exec_offset_limit ORDER BY title ASC LIMIT 2 OFFSET 2",
                    vec![],
                )
                .expect("query should execute");

            // Assert
            assert_eq!(result.rows.len(), 2);
            let ids = result
                .rows
                .into_iter()
                .map(|row| match &row[0] {
                    Value::String(value) => value.clone(),
                    _ => panic!("expected id string"),
                })
                .collect::<Vec<_>>();
            assert_eq!(ids, vec!["d3".to_string(), "d4".to_string()]);
        });
    }

    #[test]
    fn should_default_missing_offset_to_zero_in_execution() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_default_offset";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "c"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d2".to_string()),
                    serde_json::json!({"title": "a"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d3".to_string()),
                    serde_json::json!({"title": "b"}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let default_offset_result = cassie
                .execute_sql(
                    &session,
                    "SELECT id FROM exec_default_offset ORDER BY title ASC LIMIT 1",
                    vec![],
                )
                .expect("query should execute");

            let explicit_offset_result = cassie
                .execute_sql(
                    &session,
                    "SELECT id FROM exec_default_offset ORDER BY title ASC LIMIT 1 OFFSET 0",
                    vec![],
                )
                .expect("query should execute");

            // Assert
            assert_eq!(default_offset_result.rows.len(), 1);
            assert_eq!(explicit_offset_result.rows.len(), 1);
            assert_eq!(default_offset_result.rows, explicit_offset_result.rows);
        });
    }

    #[test]
    fn should_cleanup_parallel_aggregation_workers_on_timeout() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_aggregation_timeout");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_aggregation_workers = 4;
        config.limits.query_timeout_ms = 1;
        let controls = QueryExecutionControls::from_limits(
            &config.limits,
            Instant::now()
                .checked_sub(Duration::from_secs(1))
                .expect("expired query start"),
        );
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let collection = "exec_parallel_aggregation_timeout";
            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "category".to_string(),
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
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            let documents = (0..1024)
                .map(|index| {
                    (
                        Some(format!("doc-{index:04}")),
                        serde_json::json!({
                            "category": format!("g{}", index % 4),
                            "score": 1,
                        }),
                    )
                })
                .collect::<Vec<_>>();
            cassie.midge.put_documents(collection, documents).unwrap();
            let parsed = parser::parse_statement(
            "SELECT category, SUM(score) FROM exec_parallel_aggregation_timeout GROUP BY category",
        )
        .expect("parse aggregate");
            let bound = binder::bind(parsed, &cassie.catalog).expect("bind aggregate");
            let logical = cassie::planner::logical::plan(&bound).expect("plan aggregate");
            let physical = Arc::new(cassie::planner::physical::build(logical));

            // Act
            let result = executor::run_with_controls(&cassie, &physical, vec![], &controls);
            let metrics = cassie.metrics();

            // Assert
            let message = result
                .expect_err("parallel aggregate should time out")
                .to_string();
            assert!(
                message.contains("query timeout exceeded"),
                "expected timeout error, got {message}"
            );
            assert_eq!(
                metrics["parallel_aggregation"]["aggregations"].as_u64(),
                Some(0)
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/executor_parallel.rs.
mod executor_parallel {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
    use cassie::executor;
    use cassie::planner::logical::LogicalPlan;
    use cassie::planner::physical::PhysicalPlan;
    use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
    use cassie::sql::binder;
    use cassie::sql::parser;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    use super::support_executor as support;
    use support::*;

    const PARALLEL_ROW_COUNT: usize = 1025;

    fn parallel_row_count_i64() -> i64 {
        i64::try_from(PARALLEL_ROW_COUNT).expect("parallel row count should fit i64")
    }

    fn usize_to_i64(value: usize) -> i64 {
        i64::try_from(value).expect("test row count should fit i64")
    }

    fn create_registered_collection(
        cassie: &Cassie,
        collection: &str,
        fields: &[(&str, DataType)],
    ) {
        let schema = Schema {
            fields: fields
                .iter()
                .map(|(name, data_type)| FieldSchema {
                    name: (*name).to_string(),
                    data_type: data_type.clone(),
                    nullable: true,
                })
                .collect(),
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
    }

    fn put_documents(
        cassie: &Cassie,
        collection: &str,
        documents: impl IntoIterator<Item = (String, serde_json::Value)>,
    ) {
        cassie
            .midge
            .put_documents(
                collection,
                documents
                    .into_iter()
                    .map(|(id, payload)| (Some(id), payload))
                    .collect(),
            )
            .unwrap();
    }

    #[test]
    fn should_score_fulltext_candidates_with_parallel_workers() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_scoring_fulltext");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_scoring_workers = 4;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let collection = "exec_parallel_scoring_fulltext";
        let session = cassie.create_session("tester", None);
        create_registered_collection(
            &cassie,
            collection,
            &[("id", DataType::Text), ("body", DataType::Text)],
        );
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "id": format!("doc-{index:04}"),
                        "body": "alpha beta",
                    }),
                )
            }),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM exec_parallel_scoring_fulltext WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 3",
                vec![],
            )
            .expect("parallel scoring query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 3);
        assert!(metrics["parallel_scoring"]["scorings"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert!(metrics["parallel_scoring"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 2);
        assert!(metrics["parallel_scoring"]["rows"].as_u64().unwrap_or(0) > 0);
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_parallel_scoring_when_worker_limit_is_one() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_scoring_fallback");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_scoring_workers = 1;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_scoring_fallback (id TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO exec_parallel_scoring_fallback (id, body) VALUES ('doc-1', 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM exec_parallel_scoring_fallback WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .expect("fallback scoring query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(metrics["parallel_scoring"]["scorings"].as_u64(), Some(0));
        assert!(metrics["parallel_scoring"]["fallback_scorings"]
            .as_u64()
            .unwrap_or(0)
            > 0);
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_merge_parallel_scan_batches_deterministically() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_scan_merge");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_scan_workers = 4;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let collection = "exec_parallel_scan_merge";
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
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            put_documents(
                &cassie,
                collection,
                (0..PARALLEL_ROW_COUNT).map(|index| {
                    (
                        format!("doc-{index:04}"),
                        serde_json::json!({
                            "id": format!("doc-{index:04}"),
                            "title": format!("title-{index:04}"),
                        }),
                    )
                }),
            );
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
                .execute_sql(&session, "SELECT * FROM exec_parallel_scan_merge", vec![])
                .expect("parallel scan query should execute");
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(result.rows.len(), PARALLEL_ROW_COUNT);
            assert!(metrics["parallel_scans"]["scans"].as_u64().unwrap_or(0) > 0);
            assert!(metrics["parallel_scans"]["workers"].as_u64().unwrap_or(0) >= 2);
            assert_eq!(
                metrics["parallel_scans"]["rows"].as_u64(),
                Some(PARALLEL_ROW_COUNT as u64)
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_parallel_scan_when_worker_limit_is_one() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_scan_single_worker");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_scan_workers = 1;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let collection = "exec_parallel_scan_single_worker";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };
            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM exec_parallel_scan_single_worker",
                    vec![],
                )
                .expect("single-worker query should execute");
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);
            assert_eq!(metrics["parallel_scans"]["scans"].as_u64(), Some(0));
            assert!(
                metrics["parallel_scans"]["fallback_scans"]
                    .as_u64()
                    .unwrap_or(0)
                    > 0
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_execute_grouped_aggregates_with_parallel_workers() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_aggregation_grouped");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_aggregation_workers = 4;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let collection = "exec_parallel_aggregation_grouped";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "category".to_string(),
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
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let category = if index % 2 == 0 { "even" } else { "odd" };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "category": category,
                        "score": 1,
                    }),
                )
            }),
        );
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total, SUM(score) AS sum_score FROM exec_parallel_aggregation_grouped GROUP BY category ORDER BY category",
                vec![],
            )
            .expect("parallel aggregate query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::String("even".to_string()),
                    Value::Int64(513),
                    Value::Int64(513),
                ],
                vec![
                    Value::String("odd".to_string()),
                    Value::Int64(512),
                    Value::Int64(512),
                ],
            ]
        );
        assert!(metrics["parallel_aggregation"]["aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert!(metrics["parallel_aggregation"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 2);
        assert_eq!(
            metrics["parallel_aggregation"]["rows"].as_u64(),
            Some(PARALLEL_ROW_COUNT as u64)
        );
        assert_eq!(metrics["parallel_aggregation"]["groups"].as_u64(), Some(2));
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_execute_ungrouped_parallel_aggregates_with_nulls() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_aggregation_ungrouped_nulls");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_aggregation_workers = 4;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let collection = "exec_parallel_aggregation_nulls";
        let session = cassie.create_session("tester", None);
        create_registered_collection(&cassie, collection, &[("score", DataType::Int)]);
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let score = if index % 10 == 0 {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(1)
                };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({ "score": score }),
                )
            }),
        );
        let non_null_rows = (0..PARALLEL_ROW_COUNT)
            .filter(|index| index % 10 != 0)
            .count();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) AS total, COUNT(score) AS non_null, SUM(score) AS sum_score, AVG(score) AS avg_score, MIN(score) AS min_score, MAX(score) AS max_score FROM exec_parallel_aggregation_nulls",
                vec![],
            )
            .expect("parallel ungrouped aggregate should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::Int64(parallel_row_count_i64()),
                Value::Int64(usize_to_i64(non_null_rows)),
                Value::Int64(usize_to_i64(non_null_rows)),
                Value::Float64(1.0),
                Value::Int64(1),
                Value::Int64(1),
            ]]
        );
        assert!(metrics["parallel_aggregation"]["aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_having_order_limit_offset_for_parallel_aggregation() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_aggregation_having_order");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_aggregation_workers = 4;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let collection = "exec_parallel_aggregation_having";
        let session = cassie.create_session("tester", None);
        create_registered_collection(
            &cassie,
            collection,
            &[("category", DataType::Text), ("score", DataType::Int)],
        );
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let category = match index % 4 {
                    0 => "a",
                    1 => "b",
                    2 => "c",
                    _ => "d",
                };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "category": category,
                        "score": 1,
                    }),
                )
            }),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total FROM exec_parallel_aggregation_having GROUP BY category HAVING COUNT(*) >= 256 ORDER BY category LIMIT 2 OFFSET 1",
                vec![],
            )
            .expect("parallel aggregate query should preserve downstream clauses");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("b".to_string()), Value::Int64(256)],
                vec![Value::String("c".to_string()), Value::Int64(256)],
            ]
        );
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_report_parallel_integer_sum_merge_overflow() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_aggregation_sum_overflow");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_aggregation_workers = 4;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "exec_parallel_aggregation_overflow";
        let session = cassie.create_session("tester", None);
        create_registered_collection(&cassie, collection, &[("amount", DataType::BigInt)]);
        let partial_safe_amount = i64::MAX / 1024;
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({ "amount": partial_safe_amount }),
                )
            }),
        );

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT SUM(amount) FROM exec_parallel_aggregation_overflow",
                vec![],
            )
            .expect_err("parallel aggregate merge should report overflow");

        // Assert
        assert!(error.to_string().contains("aggregate integer overflow"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_parallel_aggregation_for_distinct() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_aggregation_distinct_fallback");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_aggregation_workers = 4;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let collection = "exec_parallel_aggregation_distinct";
        let session = cassie.create_session("tester", None);
        create_registered_collection(&cassie, collection, &[("category", DataType::Text)]);
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let category = if index % 2 == 0 { "even" } else { "odd" };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({ "category": category }),
                )
            }),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT category, COUNT(*) AS total FROM exec_parallel_aggregation_distinct GROUP BY category ORDER BY category",
                vec![],
            )
            .expect("distinct aggregate fallback should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(
            metrics["parallel_aggregation"]["aggregations"].as_u64(),
            Some(0)
        );
        assert!(metrics["parallel_aggregation"]["fallback_aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert_eq!(
            metrics["parallel_aggregation"]["last_fallback_reason"].as_str(),
            Some("distinct")
        );
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_parallel_aggregation_when_worker_limit_is_one() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_aggregation_single_worker");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_aggregation_workers = 1;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_aggregation_single_worker (score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO exec_parallel_aggregation_single_worker (score) VALUES (7)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) AS total, SUM(score) AS sum_score FROM exec_parallel_aggregation_single_worker",
                vec![],
            )
            .expect("single-worker aggregate should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Int64(1), Value::Int64(7)]]);
        assert_eq!(
            metrics["parallel_aggregation"]["aggregations"].as_u64(),
            Some(0)
        );
        assert!(metrics["parallel_aggregation"]["fallback_aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert_eq!(
            metrics["parallel_aggregation"]["last_fallback_reason"].as_str(),
            Some("worker-limit-one")
        );
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_parallel_aggregation_for_user_defined_function() {
        // Arrange
        use_local_storage();
        let path = data_dir("parallel_aggregation_udf_fallback");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.parallel_aggregation_workers = 4;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let collection = "exec_parallel_aggregation_udf";
            let session = cassie.create_session("tester", None);
            create_registered_collection(&cassie, collection, &[("score", DataType::Int)]);
            cassie
                .execute_sql(
                    &session,
                    "CREATE FUNCTION agg_identity(x INT) RETURNS INT AS \"x\"",
                    vec![],
                )
                .unwrap();
            put_documents(
                &cassie,
                collection,
                (0..PARALLEL_ROW_COUNT)
                    .map(|index| (format!("doc-{index:04}"), serde_json::json!({ "score": 1 }))),
            );

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT SUM(agg_identity(score)) AS total FROM exec_parallel_aggregation_udf",
                    vec![],
                )
                .expect("udf aggregate fallback should execute");
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows,
                vec![vec![Value::Int64(parallel_row_count_i64())]]
            );
            assert_eq!(
                metrics["parallel_aggregation"]["aggregations"].as_u64(),
                Some(0)
            );
            assert!(
                metrics["parallel_aggregation"]["fallback_aggregations"]
                    .as_u64()
                    .unwrap_or(0)
                    > 0
            );
            assert_eq!(
                metrics["parallel_aggregation"]["last_fallback_reason"].as_str(),
                Some("unsupported-expression")
            );
        });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/executor_projection.rs.
mod executor_projection {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
    use cassie::executor;
    use cassie::planner::logical::LogicalPlan;
    use cassie::planner::physical::PhysicalPlan;
    use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
    use cassie::sql::binder;
    use cassie::sql::parser;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    use super::support_executor as support;
    use support::*;

    #[test]
    fn should_execute_simple_filtered_query() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let path = data_dir("smoke");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "exec_smoke";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(collection, None, serde_json::json!({"title": "alpha"}))
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM exec_smoke WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(result.columns[0].name, "title");
            assert_eq!(result.rows.len(), 1);
            match &result.rows[0][0] {
                Value::String(value) => assert_eq!(value, "alpha"),
                _ => panic!("expected string in first column"),
            }

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_query_across_multiple_batches_without_truncation() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_multi_batch";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            let documents = (0..1029)
                .map(|index| {
                    let id = format!("d{index:04}");
                    let title = format!("doc-{index:04}");
                    (Some(id), serde_json::json!({ "title": title }))
                })
                .collect::<Vec<_>>();
            cassie.midge.put_documents(collection, documents).unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id FROM exec_multi_batch ORDER BY title ASC LIMIT 5 OFFSET 1024",
                    vec![],
                )
                .expect("query should execute");

            // Assert
            assert_eq!(result.rows.len(), 5);
            let ids = result
                .rows
                .into_iter()
                .map(|row| match &row[0] {
                    Value::String(value) => value.clone(),
                    _ => panic!("expected id string"),
                })
                .collect::<Vec<_>>();
            assert_eq!(
                ids,
                vec![
                    "d1024".to_string(),
                    "d1025".to_string(),
                    "d1026".to_string(),
                    "d1027".to_string(),
                    "d1028".to_string(),
                ]
            );
        });
    }

    #[test]
    fn should_preserve_filtered_projection_across_multiple_batches() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_multi_batch_filter";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "status".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

        let documents = (0..1030)
            .map(|index| {
                let id = format!("d{index:04}");
                let title = format!("doc-{index:04}");
                let status = if index % 2 == 0 { "keep" } else { "drop" };
                (
                    Some(id),
                    serde_json::json!({ "title": title, "status": status }),
                )
            })
            .collect::<Vec<_>>();
        cassie.midge.put_documents(collection, documents).unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_multi_batch_filter WHERE status = 'keep' ORDER BY title ASC LIMIT 5 OFFSET 510",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 5);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id string"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "d1020".to_string(),
                "d1022".to_string(),
                "d1024".to_string(),
                "d1026".to_string(),
                "d1028".to_string(),
            ]
        );
    });
    }

    #[test]
    fn should_project_missing_columns_as_null() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_missing_projection_column";

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

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT title, body FROM exec_missing_projection_column",
                    vec![],
                )
                .expect("query should execute");

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(result.rows[0].len(), 2);
            assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
            assert_eq!(result.rows[0][1], Value::Null);
        });
    }

    #[test]
    fn should_project_complex_values_through_filtered_ordered_scan() {
        // Arrange
        use_local_storage();
        let path = data_dir("zero_copy_projected_complex_values");
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
                "CREATE TABLE zero_copy_projected_complex_values (title TEXT, score INT, payload JSON, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        let collection = cassie
            .catalog
            .get_schema("zero_copy_projected_complex_values")
            .expect("collection schema")
            .collection;
        cassie
            .midge
            .put_document(
                &collection,
                Some("doc-1".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "score": 2,
                    "payload": {"nested": ["a", "b"]},
                    "embedding": [1.0, 2.0],
                }),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                &collection,
                Some("doc-2".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "score": 1,
                    "embedding": [3.0, 4.0],
                }),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT payload, embedding FROM zero_copy_projected_complex_values WHERE title = 'alpha' ORDER BY score ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::Null);
        assert_eq!(
            result.rows[0][1],
            Value::Vector(cassie::types::Vector::new(vec![3.0, 4.0]))
        );
        assert_eq!(
            result.rows[1][0],
            Value::Json(serde_json::json!({"nested": ["a", "b"]}))
        );
        assert_eq!(
            result.rows[1][1],
            Value::Vector(cassie::types::Vector::new(vec![1.0, 2.0]))
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/executor_query_sources.rs.
mod executor_query_sources {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
    use cassie::executor;
    use cassie::planner::logical::LogicalPlan;
    use cassie::planner::physical::PhysicalPlan;
    use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
    use cassie::sql::binder;
    use cassie::sql::parser;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    use super::support_executor as support;
    use support::*;

    fn seed_alias_query_docs(cassie: &Cassie, collection: &str) {
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
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        for (title, body, embedding) in [
            ("alpha", "world news", [1.0, 2.0]),
            ("beta", "world peace", [0.5, 1.5]),
            ("gamma", "misc", [2.0, 0.5]),
        ] {
            cassie
                .midge
                .put_document(
                    collection,
                    None,
                    serde_json::json!({
                        "title": title,
                        "body": body,
                        "embedding": embedding,
                    }),
                )
                .unwrap();
        }
    }

    fn assert_alias_query_result(result: executor::QueryResult) {
        assert_eq!(result.columns[0].name, "title_out");
        assert_eq!(result.columns[1].name, "score");
        assert_eq!(result.rows.len(), 2);
        for row in result.rows {
            assert_eq!(row.len(), 2);
            match &row[1] {
                Value::Float64(_) => {}
                _ => panic!("expected float score"),
            }
        }
    }

    fn assert_parameterized_alias_result(result: &executor::QueryResult) {
        assert_eq!(result.rows.len(), 1);
        match &result.rows[0][0] {
            Value::String(value) => assert_eq!(value, "alpha"),
            _ => panic!("expected string in first column"),
        }
    }

    #[test]
    fn should_execute_query_with_alias_filters() {
        // Arrange
        // Act
        // Assert
        use_local_storage();
        let mut path = std::env::temp_dir();
        path.push(format!("cassie-exec-{}", Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "exec_docs_alias";
        seed_alias_query_docs(&cassie, collection);

        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            "SELECT title AS title_out, search_score(body, 'world') AS score FROM exec_docs_alias WHERE body LIKE '%world%' OR title = 'gamma' ORDER BY score DESC, id ASC LIMIT 2",
            vec![],
        )
        .expect("query should execute");
        assert_alias_query_result(result);

        let params = vec![Value::String("alpha".to_string())];
        let parsed =
            parser::parse_statement("SELECT title FROM exec_docs_alias WHERE title = $1").unwrap();
        binder::bind(parsed, &cassie.catalog).unwrap();
        let param_result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM exec_docs_alias WHERE title = $1",
                params,
            )
            .expect("parameterized query should run");
        assert_parameterized_alias_result(&param_result);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_execute_query_respects_boolean_precedence() {
        // Arrange
        // Act
        // Assert
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_precedence";

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

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "x"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "x"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "beta", "body": "y"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d4".to_string()),
                serde_json::json!({"title": "gamma", "body": "x"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM exec_precedence WHERE title = 'alpha' OR title = 'beta' AND body = 'x' ORDER BY id",
            vec![],
        )

.expect("query should execute");

        assert_eq!(result.rows.len(), 2);

        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id value"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["d1".to_string(), "d2".to_string()]);
    }

    #[test]
    fn should_execute_query_parentheses_override_precedence() {
        // Arrange
        // Act
        // Assert
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_precedence_paren";

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

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "x"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "x"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "beta", "body": "y"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d4".to_string()),
                serde_json::json!({"title": "gamma", "body": "x"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM exec_precedence_paren WHERE (title = 'alpha' OR title = 'beta') AND body = 'x' ORDER BY id",
            vec![],
        )

.expect("query should execute");

        assert_eq!(result.rows.len(), 2);

        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id value"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["d1".to_string(), "d2".to_string()]);
    }

    #[test]
    fn should_execute_query_with_non_recursive_cte() {
        // Arrange
        // Act
        // Assert
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_cte_simple";

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

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "first"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "second"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            "WITH docs_cte AS (SELECT title FROM exec_cte_simple) SELECT title FROM docs_cte ORDER BY title",
            vec![],
        )

.unwrap();

        assert_eq!(result.columns[0].name, "title");
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
        assert_eq!(result.rows[1][0], Value::String("beta".to_string()));
    }

    #[test]
    fn should_execute_query_with_ordered_cte_dependencies() {
        // Arrange
        // Act
        // Assert
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_cte_dependency";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            "WITH first AS (SELECT title FROM exec_cte_dependency), second AS (SELECT title FROM first WHERE title = 'beta') SELECT title FROM second",
            vec![],
        )

.unwrap();

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("beta".to_string()));
    }

    #[test]
    fn should_execute_query_passes_params_to_cte_main_query() {
        // Arrange
        // Act
        // Assert
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_cte_params";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            "WITH filtered_docs AS (SELECT title FROM exec_cte_params WHERE title = $1) SELECT title FROM filtered_docs WHERE title = $1",
            vec![Value::String("alpha".to_string())],
        )

.unwrap();

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
    }

    #[test]
    fn should_execute_recursive_cte_until_stabilization() {
        // Arrange
        // Act
        // Assert
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_cte_recursive";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "n".to_string(),
                data_type: DataType::Int,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"n": 1}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT n FROM exec_cte_recursive WHERE n = 1 UNION ALL SELECT CAST(n + 1 AS INT) FROM seq WHERE n < 2) SELECT n FROM seq ORDER BY n",
            vec![],
        )

.unwrap();

        let values = result
            .rows
            .into_iter()
            .map(|row| match row.first() {
                Some(Value::Int64(value)) => *value,
                _ => panic!("expected integer value"),
            })
            .collect::<Vec<_>>();
        assert_eq!(values, vec![1, 2]);
    }

    #[test]
    fn should_execute_recursive_cte_enforces_depth_limit_when_no_stabilization() {
        // Arrange
        // Act
        // Assert
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_cte_infinite";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "n".to_string(),
                data_type: DataType::Int,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"n": 1}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT n FROM exec_cte_infinite WHERE n = 1 UNION ALL SELECT n + 1 AS n FROM seq) SELECT n FROM seq",
            vec![],
        )
        ;

        assert!(result.is_err());
    }
}

// Formerly tests/executor_sort.rs.
mod executor_sort {
    #![allow(unused_imports, dead_code)]

    use super::support_executor as support;
    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
    use cassie::executor;
    use cassie::planner::logical::LogicalPlan;
    use cassie::planner::physical::PhysicalPlan;
    use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
    use cassie::sql::binder;
    use cassie::sql::parser;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    use support::*;

    #[test]
    fn should_sort_with_stable_tiebreaker() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_stable_tie";

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

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("z".to_string()),
                    serde_json::json!({"title": "same", "body": "value"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("a".to_string()),
                    serde_json::json!({"title": "same", "body": "value"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("m".to_string()),
                    serde_json::json!({"title": "same", "body": "value"}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id FROM exec_stable_tie ORDER BY 1 ASC",
                    vec![],
                )
                .expect("query should execute");

            // Assert
            assert_eq!(result.rows.len(), 3);
            let ids = result
                .rows
                .into_iter()
                .map(|row| match &row[0] {
                    Value::String(value) => value.clone(),
                    _ => panic!("expected id string"),
                })
                .collect::<Vec<_>>();
            assert_eq!(ids, vec!["a".to_string(), "m".to_string(), "z".to_string()]);
        });
    }

    #[test]
    fn should_sort_by_projection_alias_with_different_case() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_hybrid_alias_case";

            let schema = Schema {
                fields: vec![
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "embedding".to_string(),
                        data_type: DataType::Vector(2),
                        nullable: true,
                    },
                ],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())

                .unwrap();
            cassie
                .register_collection(
                    collection,
                    schema
                        .fields
                        .iter()
                        .map(|field| (field.name.clone(), field.data_type.clone()))
                        .collect(),
                );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("z".to_string()),
                    serde_json::json!({"body": "red", "embedding": [10.0, 0.0]}),
                )

                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("a".to_string()),
                    serde_json::json!({"body": "red", "embedding": [1.0, 0.0]}),
                )

                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("m".to_string()),
                    serde_json::json!({"body": "red", "embedding": [0.0, 1.0]}),
                )

                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS Score FROM exec_hybrid_alias_case ORDER BY SCORE DESC",
                    vec![],
                )

    .expect("query should execute");

            // Assert
            assert_eq!(result.rows.len(), 3);
            let ids = result
                .rows
                .into_iter()
                .map(|row| match &row[0] {
                    Value::String(value) => value.clone(),
                    _ => panic!("expected id"),
                })
                .collect::<Vec<_>>();
            assert_eq!(ids, vec!["a".to_string(), "m".to_string(), "z".to_string()]);
        });
    }

    #[test]
    fn should_sort_shadowing_alias_independently_of_source_nulls() {
        // Arrange
        let path = data_dir("order_alias_shadow");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE order_alias_shadow (a INT, b INT)",
                    vec![],
                )
                .expect("create table");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO order_alias_shadow (a, b) VALUES (5, 1), (NULL, 2), (3, 3)",
                    vec![],
                )
                .expect("seed table");

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT b AS a FROM order_alias_shadow ORDER BY a",
                    vec![],
                )
                .expect("sort by shadowing alias");

            // Assert
            assert_eq!(
                result.rows,
                vec![
                    vec![Value::Int64(1)],
                    vec![Value::Int64(2)],
                    vec![Value::Int64(3)],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_sort_by_unprojected_column_before_projection() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_order_by_unprojected_field";

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

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("id1".to_string()),
                    serde_json::json!({"title": "title-a", "body": "zzz"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("id2".to_string()),
                    serde_json::json!({"title": "title-b", "body": "aaa"}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM exec_order_by_unprojected_field ORDER BY body ASC",
                    vec![],
                )
                .expect("query should execute");

            // Assert
            assert_eq!(result.rows.len(), 2);
            assert_eq!(result.rows[0][0], Value::String("title-b".to_string()));
            assert_eq!(result.rows[1][0], Value::String("title-a".to_string()));
        });
    }

    #[test]
    fn should_be_deterministic_for_repeated_execution_metadata() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_repeated_metadata";

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

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("id1".to_string()),
                    serde_json::json!({"title": "alpha", "body": "first"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("id2".to_string()),
                    serde_json::json!({"title": "beta", "body": "second"}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let first = cassie
                .execute_sql(
                    &session,
                    "SELECT title, body FROM exec_repeated_metadata ORDER BY title ASC",
                    vec![],
                )
                .expect("query should execute");
            let second = cassie
                .execute_sql(
                    &session,
                    "SELECT title, body FROM exec_repeated_metadata ORDER BY title ASC",
                    vec![],
                )
                .expect("query should execute");

            // Assert
            assert_eq!(first.command, second.command);
            let first_columns = first
                .columns
                .iter()
                .map(|column| (column.name.clone(), column.data_type.clone()))
                .collect::<Vec<_>>();
            let second_columns = second
                .columns
                .iter()
                .map(|column| (column.name.clone(), column.data_type.clone()))
                .collect::<Vec<_>>();
            assert_eq!(first_columns, second_columns);
            assert_eq!(first.rows, second.rows);
        });
    }
}
// Formerly tests/executor_streaming.rs.
mod executor_streaming {
    use super::support_executor as support;
    use cassie::app::{Cassie, CassieError};
    use cassie::config::CassieRuntimeConfig;
    use cassie::types::Value;
    use support::*;

    #[test]
    fn should_stop_collection_scan_after_limit_is_satisfied() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("streaming-limit");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE streaming_limit (id TEXT, payload TEXT)",
                vec![],
            )
            .expect("create table");
        for index in 0..50 {
            cassie
                .midge
                .put_document(
                    "streaming_limit",
                    Some(format!("doc-{index:02}")),
                    serde_json::json!({"id": format!("doc-{index:02}"), "payload": "value"}),
                )
                .expect("seed row");
        }
        let before = cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT payload FROM streaming_limit LIMIT 1",
                vec![],
            )
            .expect("bounded query");
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("value".to_string())]]);
        assert_eq!(visited, 1, "LIMIT 1 should visit exactly one stored row");

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_stop_collection_scan_when_result_row_cap_is_exceeded() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("streaming-result-cap");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.max_result_rows = 1;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE streaming_result_cap (id TEXT, payload TEXT)",
                vec![],
            )
            .expect("create table");
        for index in 0..50 {
            cassie
                .midge
                .put_document(
                    "streaming_result_cap",
                    Some(format!("doc-{index:02}")),
                    serde_json::json!({"id": format!("doc-{index:02}"), "payload": "value"}),
                )
                .expect("seed row");
        }
        let before = cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let error = cassie
            .execute_sql(&session, "SELECT payload FROM streaming_result_cap", vec![])
            .expect_err("result cap should reject the second row");
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert_eq!(visited, 2, "row cap should stop after the first excess row");

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_stop_collection_scan_after_exists_finds_a_row() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("streaming-exists");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE streaming_exists (id TEXT, payload TEXT)",
                vec![],
            )
            .expect("create inner table");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE streaming_exists_outer (id TEXT, payload TEXT)",
                vec![],
            )
            .expect("create outer table");
        cassie
            .midge
            .put_document(
                "streaming_exists_outer",
                Some("outer".to_string()),
                serde_json::json!({"id": "outer", "payload": "outer"}),
            )
            .expect("seed outer row");
        for index in 0..50 {
            cassie
                .midge
                .put_document(
                    "streaming_exists",
                    Some(format!("doc-{index:02}")),
                    serde_json::json!({"id": format!("doc-{index:02}"), "payload": "value"}),
                )
                .expect("seed row");
        }
        let before = cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "SELECT payload FROM streaming_exists_outer WHERE EXISTS(SELECT payload FROM streaming_exists)",
            vec![],
        )
        .expect("exists query");
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("outer".to_string())]]);
        assert_eq!(visited, 2, "EXISTS should stop after its first inner row");

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/executor_vector_scoring.rs.
mod executor_vector_scoring {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
    use cassie::executor;
    use cassie::planner::logical::LogicalPlan;
    use cassie::planner::physical::PhysicalPlan;
    use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
    use cassie::sql::binder;
    use cassie::sql::parser;
    use cassie::types::{DataType, FieldSchema, Schema, Value};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    use super::support_executor as support;
    use support::*;

    #[test]
    fn should_execute_query_filters_by_vector_score_function() {
        // Arrange
        // Act
        // Assert
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_vector_score_filter";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [0.0, 1.0]}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let result = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_score(embedding, '[1,0]') AS score FROM exec_vector_score_filter WHERE vector_score(embedding, '[1,0]') > 0.5",
            vec![],
        )

.expect("query should execute");

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.columns[0].name, "id");
        assert_eq!(result.columns[1].name, "score");
        assert_eq!(
            result.rows[0][0],
            cassie::types::Value::String("d1".to_string())
        );
    }

    #[test]
    fn should_execute_query_orders_by_vector_distance_function_parameterized() {
        // Arrange
        // Act
        // Assert
        use_local_storage();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_vector_order_func";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [0.2, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"embedding": [10.0, 10.0]}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let params = vec![cassie::types::Value::String("[1,0]".to_string())];
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_vector_order_func ORDER BY vector_distance(embedding, $1) ASC",
                params,
            )
            .expect("query should execute");

        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                cassie::types::Value::String(id) => id.clone(),
                _ => panic!("expected string id"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["d1".to_string(), "d2".to_string(), "d3".to_string()]
        );
    }

    #[test]
    fn should_order_by_pgvector_dot_operator() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_vector_dot_order";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"embedding": [1.0, 0.0]}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d2".to_string()),
                    serde_json::json!({"embedding": [2.0, 0.0]}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d3".to_string()),
                    serde_json::json!({"embedding": [0.0, 2.0]}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id FROM exec_vector_dot_order ORDER BY embedding <#> '[1,0]' ASC",
                    vec![],
                )
                .expect("query should execute");

            // Assert
            assert_eq!(result.rows.len(), 3);
            let ids = result
                .rows
                .into_iter()
                .map(|row| match &row[0] {
                    Value::String(value) => value.clone(),
                    _ => panic!("expected id string"),
                })
                .collect::<Vec<_>>();
            assert_eq!(
                ids,
                vec!["d2".to_string(), "d1".to_string(), "d3".to_string()]
            );
        });
    }

    #[test]
    fn should_order_by_pgvector_l2_operator() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_vector_l2_order";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"embedding": [1.0, 0.0]}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d2".to_string()),
                    serde_json::json!({"embedding": [2.0, 0.0]}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("d3".to_string()),
                    serde_json::json!({"embedding": [0.0, 2.0]}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT id FROM exec_vector_l2_order ORDER BY embedding <-> '[1,0]' ASC",
                    vec![],
                )
                .expect("query should execute");

            // Assert
            assert_eq!(result.rows.len(), 3);
            let ids = result
                .rows
                .into_iter()
                .map(|row| match &row[0] {
                    Value::String(value) => value.clone(),
                    _ => panic!("expected id string"),
                })
                .collect::<Vec<_>>();
            assert_eq!(
                ids,
                vec!["d1".to_string(), "d2".to_string(), "d3".to_string()]
            );
        });
    }

    #[test]
    fn should_fail_query_when_vector_function_dimensions_mismatch() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            use_local_storage();
            let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
            let collection = "exec_vector_mismatch";

            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

            cassie
                .midge
                .put_document(
                    collection,
                    Some("d1".to_string()),
                    serde_json::json!({"embedding": [1.0, 2.0]}),
                )
                .unwrap();

            // Act
            let session = cassie.create_session("tester", None);
            let result = cassie.execute_sql(
                &session,
                "SELECT vector_distance(embedding, '[1,0,0]') FROM exec_vector_mismatch",
                vec![],
            );

            // Assert
            assert!(result.is_err());
        });
    }

    #[test]
    fn should_execute_create_vector_index_command() {
        // Arrange
        use_local_storage();
        let path = data_dir("ddl_vector_index_create_command");
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE idx_vector_commands (id TEXT, content TEXT, embedding VECTOR(1536))",
                vec![],
            )

.unwrap();

        let create_index = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_vector_embedding ON idx_vector_commands USING vector (embedding) WITH (source_field = content, metric = l2)",
                vec![],
            )
            .unwrap();

        let catalog_index = cassie
            .catalog
            .get_index("idx_vector_commands", "idx_vector_embedding")
            .expect("index should be in catalog");
        let stored_vector = cassie
            .midge
            .get_vector_index("idx_vector_commands", "embedding")

            .unwrap()
            .expect("vector index should be persisted");

        // Assert
        assert_eq!(create_index.command, "CREATE INDEX");
        assert_eq!(create_index.columns.len(), 0);
        assert!(matches!(catalog_index.kind, IndexKind::Vector));
        assert_eq!(catalog_index.field, "embedding");
        assert_eq!(catalog_index.fields, vec!["embedding".to_string()]);
        assert_eq!(
            catalog_index.options.get("source_field"),
            Some(&"content".to_string())
        );
        assert_eq!(catalog_index.options.get("metric"), Some(&"l2".to_string()));
        assert_eq!(stored_vector.field, "embedding");
        assert_eq!(stored_vector.source_field, "content");
        assert_eq!(stored_vector.metadata.metric, DistanceMetric::L2);
        assert_eq!(
            stored_vector.metadata.provider,
            cassie.embedding_provider.provider_name()
        );
        assert_eq!(
            stored_vector.metadata.model,
            cassie.embedding_provider.model_name().to_string()
        );
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_invalid_hnsw_vector_index_options() {
        // Arrange
        use_local_storage();
        let path = data_dir("ddl_vector_index_invalid_hnsw");
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE idx_vector_bad_hnsw (content TEXT, embedding VECTOR(1536))",
                vec![],
            )
            .unwrap();

        // Act
        let err = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_vector_bad_hnsw_embedding ON idx_vector_bad_hnsw USING vector (embedding) WITH (source_field = content, index_type = hnsw, m = 1)",
                vec![],
            )
            .expect_err("invalid hnsw options should fail");

        // Assert
        assert!(err
            .to_string()
            .contains("vector index option 'm' must be in [2, 128]"));
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_execute_drop_vector_index_command() {
        // Arrange
        use_local_storage();
        let path = data_dir("ddl_vector_index_drop_command");
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let session = cassie.create_session("tester", None);

        // Arrange
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE idx_vector_commands (id TEXT, content TEXT, embedding VECTOR(1536))",
                vec![],
            )

.unwrap();

        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_vector_embedding ON idx_vector_commands USING vector (embedding) WITH (source_field = content, metric = l2)",
                vec![],
            )
            .unwrap();

        // Act
        let drop_index = cassie
            .execute_sql(
                &session,
                "DROP INDEX idx_vector_embedding ON idx_vector_commands",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(drop_index.command, "DROP INDEX");
        assert!(cassie
            .catalog
            .get_index("idx_vector_commands", "idx_vector_embedding")
            .is_none());
        assert!(cassie
            .midge
            .get_vector_index("idx_vector_commands", "embedding")

            .unwrap()
            .is_none());
    });

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/procedure_support_contract.rs.
mod procedure_support_contract {
    #[test]
    fn should_publish_the_narrow_procedure_support_contract() {
        // Arrange
        let contract = include_str!("../docs/procedure-support.md");
        let feature_support = include_str!("../docs/feature-support.md");

        // Act
        let required_boundaries = [
            "single Cassie SQL statement",
            "positional argument binding",
            "restart hydration",
            "tokio-postgres",
            "PL/pgSQL",
            "triggers",
            "dynamic SQL",
            "transaction control",
            "recursion",
            "business-logic platform",
        ];
        let missing = required_boundaries
            .into_iter()
            .filter(|boundary| !contract.contains(boundary))
            .collect::<Vec<_>>();

        // Assert
        assert!(
            missing.is_empty(),
            "missing procedure boundaries: {missing:?}"
        );
        assert!(feature_support.contains("| Limited procedures and `CALL`"));
    }

    #[test]
    fn should_reject_unsupported_procedural_surfaces() {
        // Arrange
        let mut path = std::env::temp_dir();
        path.push(format!("cassie-procedure-boundary-{}", Uuid::new_v4()));
        let cassie = Cassie::new_with_data_dir_and_config(&path, CassieRuntimeConfig::default())
            .expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        let unsupported = [
            r"CREATE PROCEDURE procedural() LANGUAGE plpgsql AS 'BEGIN NULL; END'",
            "CREATE TRIGGER procedure_trigger BEFORE INSERT ON target EXECUTE PROCEDURE procedural()",
            r#"CREATE PROCEDURE dynamic_query() AS "EXECUTE 'SELECT 1'""#,
        ];

        // Act
        let errors = unsupported
            .into_iter()
            .map(|sql| {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .expect_err("unsupported procedure surface must be rejected")
                    .to_string()
            })
            .collect::<Vec<_>>();

        // Assert
        assert!(errors.iter().all(|error| !error.trim().is_empty()));
        assert!(cassie.catalog.list_procedures().is_empty());

        cassie.shutdown();
        let _ = std::fs::remove_dir_all(path);
    }
    use cassie::app::Cassie;
    use cassie::config::CassieRuntimeConfig;
    use uuid::Uuid;
}
