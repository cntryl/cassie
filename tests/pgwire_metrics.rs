use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-pgwire-metrics-{}-{}",
        label,
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

#[test]
fn should_record_pgwire_connection_metrics() {
    // Arrange
    with_fallback();
    let path = data_dir("session_messages");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env();
        config.password.clear();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().await.unwrap();

        let collection = "pgwire_metrics_docs";
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
        cassie
            .catalog
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
                serde_json::json!({"title": "alpha"}),
            )
            .await
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let lines = {
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);

            // Act
            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                b"STARTUP user=postgres database=testdb\n",
            )
            .await
            .expect("startup write");
            tokio::io::AsyncWriteExt::write_all(
                &mut write_half,
                b"QUERY SELECT title FROM pgwire_metrics_docs ORDER BY title\n",
            )
            .await
            .expect("query write");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush");

            let mut lines = Vec::new();
            loop {
                let mut line = String::new();
                let read = tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
                    .await
                    .expect("read response");
                if read == 0 {
                    break;
                }
                let trimmed = line.trim_end().to_string();
                lines.push(trimmed.clone());
                if trimmed == "READY_FOR_QUERY" {
                    break;
                }
            }

            lines
        };

        drop(socket);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let metrics = cassie.metrics().await;

        // Assert
        assert!(
            lines.iter().any(|line| line == "OK auth"),
            "pgwire startup should authenticate successfully"
        );
        assert!(
            lines.iter().any(|line| line.starts_with("ROWDESC ")),
            "pgwire query should return a row description"
        );
        assert!(
            lines.iter().any(|line| line.starts_with("DATAROW ")),
            "pgwire query should return a data row"
        );
        assert_eq!(
            metrics["pgwire"]["sessions_started_total"].as_u64(),
            Some(1)
        );
        assert_eq!(metrics["pgwire"]["auth_ok_total"].as_u64(), Some(1));
        assert_eq!(metrics["pgwire"]["simple_queries_total"].as_u64(), Some(1));
        assert_eq!(metrics["pgwire"]["active_sessions"].as_u64(), Some(0));
        assert_eq!(metrics["pgwire"]["prepared_statements"].as_u64(), Some(0));

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
