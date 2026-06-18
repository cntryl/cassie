use cassie::app::Cassie;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-metrics-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

fn startup_frame(user: &str, database: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0");
    payload.extend_from_slice(user.as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"database\0");
    payload.extend_from_slice(database.as_bytes());
    payload.push(0);
    payload.push(0);

    let mut frame = Vec::new();
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("startup payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

async fn read_auth_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, i32, Vec<u8>) {
    let mut header = [0u8; 5];
    tokio::io::AsyncReadExt::read_exact(reader, &mut header)
        .await
        .expect("read auth frame header");

    let tag = header[0];
    let len = i32::from_be_bytes(header[1..].try_into().expect("auth frame length"));
    let mut payload =
        vec![0u8; usize::try_from(len - 4).expect("non-negative auth payload length")];
    tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
        .await
        .expect("read auth frame payload");

    (tag, len, payload)
}

#[test]
fn should_report_runtime_metrics_snapshot() {
    // Arrange
    with_fallback();
    let path = data_dir("startup_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();

        let collection = "metrics_runtime_docs";
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
            .await
            .unwrap();
        cassie.register_collection(collection, schema.clone()).await;
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .await
            .unwrap();

        let session = cassie.create_session("tester", None).await;

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_runtime_docs WHERE title = 'alpha'",
                vec![],
            )
            .await
            .unwrap();
        let metrics = cassie.metrics().await;

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(metrics["ready"], serde_json::Value::Bool(true));
        assert!(
            metrics["runtime"]["startup_total"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "startup counter should be recorded"
        );
        assert!(
            metrics["runtime"]["catalog_hydration_total"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "catalog hydration counter should be recorded"
        );
        assert_eq!(metrics["query"]["count"].as_u64(), Some(1));
        assert_eq!(metrics["query"]["rows_returned_total"].as_u64(), Some(1));
        assert!(
            metrics["storage"]["schema"]["reads"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "schema storage reads should be recorded"
        );
        assert!(
            metrics["storage"]["data"]["reads"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "data storage reads should be recorded"
        );
        assert!(
            metrics["storage"]["temp"]["writes"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "temp storage writes should be recorded"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_vector_counts_for_ordered_search_expression() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_candidates");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_vector_candidates";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "embedding": [1.0, 0.0],
                }),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({
                    "title": "beta",
                    "embedding": [0.0, 1.0],
                }),
            )
            .await
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-3".to_string()),
                serde_json::json!({
                    "title": "gamma",
                    "embedding": [1.0, 1.0],
                }),
            )
            .await
            .unwrap();

        let before = cassie.metrics().await;
        let before_candidates = before["vector"]["candidate_count_total"].as_u64().unwrap_or_default();
        let before_results = before["vector"]["result_count_total"].as_u64().unwrap_or_default();

        let session = cassie.create_session("tester", None).await;
        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_vector_candidates ORDER BY embedding <-> '[1,0]' LIMIT 1",
                vec![],
            )
            .await
            .unwrap();

        let after = cassie.metrics().await;

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["vector"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            3
        );
        assert_eq!(
            after["vector"]["result_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_results,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_count_failed_scan_as_storage_read_error() {
    // Arrange
    with_fallback();
    let path = data_dir("scan_errors");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie
            .catalog
            .register_collection(
                "missing_storage_collection",
                vec![("title".to_string(), DataType::Text)],
            )
            .await;

        let before = cassie.metrics().await;
        let before_errors = before["storage"]["data"]["errors"]
            .as_u64()
            .unwrap_or_default();
        let before_reads = before["storage"]["data"]["reads"]
            .as_u64()
            .unwrap_or_default();

        let session = cassie.create_session("tester", None).await;
        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM missing_storage_collection WHERE title = 'alpha'",
                vec![],
            )
            .await;
        assert!(
            result.is_err(),
            "query should fail because collection schema is missing in storage"
        );

        let after = cassie.metrics().await;

        // Assert
        assert_eq!(
            after["storage"]["data"]["errors"]
                .as_u64()
                .unwrap_or_default()
                - before_errors,
            1
        );
        assert!(
            after["storage"]["data"]["reads"]
                .as_u64()
                .unwrap_or_default()
                > before_reads,
            "scan failure should still record the read attempt"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_track_protocol_errors_for_missing_prepared_statement_describe() {
    // Arrange
    with_fallback();
    let path = data_dir("pgwire_protocol_errors");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().await.unwrap();
        let before_protocol_errors = cassie.metrics().await["pgwire"]["protocol_errors_total"]
            .as_u64()
            .unwrap_or_default();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let mut config = cassie::config::CassieRuntimeConfig::from_env();
        config.password.clear();
        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        let startup = startup_frame("postgres", "testdb");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
            .await
            .expect("startup write");

        let auth_frame = read_auth_frame(&mut reader).await;
        assert_eq!(
            auth_frame.0, b'R',
            "startup should return an authentication response"
        );

        // Act
        tokio::io::AsyncWriteExt::write_all(&mut write_half, b"DESCRIBE missing\n")
            .await
            .expect("describe write");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush");
        let mut line = String::new();
        let read = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line),
        )
        .await
        .expect("read describe response timed out")
        .expect("read response");
        let _ = read;
        assert!(
            !line.trim_end().is_empty(),
            "describe should return an error"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        drop(socket);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let metrics = cassie.metrics().await;

        // Assert
        assert_eq!(
            metrics["pgwire"]["protocol_errors_total"]
                .as_u64()
                .unwrap_or_default()
                - before_protocol_errors,
            1,
            "missing describe statement should count as a protocol error"
        );

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
