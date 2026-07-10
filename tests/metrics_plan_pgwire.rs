#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::runtime::RuntimeFeedbackKey;
use cassie::sql::parser;
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

fn feedback_key(sql: &str, collection: &str, schema_epoch: u64) -> RuntimeFeedbackKey {
    let _ = (sql, collection, schema_epoch);
    panic!("feedback_key helper is unused in metrics_plan_pgwire");
}

fn register_feedback_collection(cassie: &Cassie, collection: &str) {
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
    cassie.register_collection(collection, schema);
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha", "body": "one"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-2".to_string()),
            serde_json::json!({"title": "beta", "body": "two"}),
        )
        .unwrap();
}

fn adaptive_candidate_config(min: usize, max: usize) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.adaptive_candidate_min = min;
    config.limits.adaptive_candidate_max = max;
    config
}

fn register_adaptive_candidate_collection(cassie: &Cassie, collection: &str) {
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
    cassie.register_collection(collection, schema);
    for (id, body) in [
        ("doc-1", "alpha shared"),
        ("doc-2", "alpha shared"),
        ("doc-3", "alpha shared"),
    ] {
        cassie
            .midge
            .put_document(
                collection,
                Some(id.to_string()),
                serde_json::json!({"body": body}),
            )
            .unwrap();
    }
}

fn describe_statement_frame(statement_name: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(b'S');
    payload.extend_from_slice(statement_name.as_bytes());
    payload.push(0);

    let mut frame = Vec::new();
    frame.push(b'D');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("describe payload size must fit into i32")
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

async fn read_wire_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, Vec<u8>) {
    let mut tag = [0u8; 1];
    tokio::io::AsyncReadExt::read_exact(reader, &mut tag)
        .await
        .expect("read frame tag");

    let mut len = [0u8; 4];
    tokio::io::AsyncReadExt::read_exact(reader, &mut len)
        .await
        .expect("read frame length");
    let len = i32::from_be_bytes(len);
    let mut payload = vec![0u8; usize::try_from(len - 4).expect("non-negative payload length")];
    if !payload.is_empty() {
        tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
            .await
            .expect("read frame payload");
    }

    (tag[0], payload)
}

async fn read_until_ready(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Vec<u8> {
    loop {
        let frame = read_wire_frame(reader).await;
        if frame.0 == b'Z' {
            return frame.1;
        }
    }
}

#[test]
fn should_report_plan_cache_metrics() {
    // Arrange
    with_fallback();
    let path = data_dir("plan_cache_metrics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_plan_cache_docs";
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
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_plan_cache_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_plan_cache_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));
        assert!(
            metrics["plan_cache"]["entries"]
                .as_u64()
                .unwrap_or_default()
                >= 1
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
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let before_protocol_errors = cassie.metrics()["pgwire"]["protocol_errors_total"]
            .as_u64()
            .unwrap_or_default();

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
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        let startup = startup_frame("postgres", "postgres");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
            .await
            .expect("startup write");

        let auth_frame = read_auth_frame(&mut reader).await;
        assert_eq!(
            auth_frame.0, b'R',
            "startup should return an authentication response"
        );
        let startup_ready = read_until_ready(&mut reader).await;
        assert_eq!(startup_ready, vec![b'I']);

        // Act
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &describe_statement_frame("missing"))
            .await
            .expect("describe write");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush");
        let response = read_wire_frame(&mut reader).await;
        assert_eq!(response.0, b'E', "describe should return an error frame");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        drop(socket);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let metrics = cassie.metrics();

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
