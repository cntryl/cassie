use super::support_pgwire as support;

use cassie::app::Cassie;
use cassie::catalog::{
    canonical_relation_name, CollectionMeta, FieldConstraint, IndexKind, IndexMeta,
};
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

fn data_dir(label: &str) -> String {
    crate::support_temp_dirs::sweep_stale_once();
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-pgwire-image-{label}-{}", Uuid::new_v4()));
    path.to_string_lossy().into_owned()
}

fn query_frame(sql: &str) -> Vec<u8> {
    let mut payload = sql.as_bytes().to_vec();
    payload.push(0);
    let mut frame = vec![b'Q'];
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("query frame length")
            .to_be_bytes(),
    );
    frame.extend(payload);
    frame
}

fn copy_data_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![b'd'];
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("copy data length")
            .to_be_bytes(),
    );
    frame.extend_from_slice(payload);
    frame
}

fn copy_done_frame() -> Vec<u8> {
    vec![b'c', 0, 0, 0, 4]
}

fn seed_database_image_fixture(cassie: &Cassie) {
    let collection = canonical_relation_name("analytics", "public", "analytics");
    cassie
        .midge
        .create_database("analytics", None)
        .expect("database");
    cassie
        .midge
        .create_collection_with_meta(
            &collection,
            &Schema {
                fields: vec![FieldSchema {
                    name: "value".to_string(),
                    data_type: DataType::Text,
                    nullable: false,
                }],
            },
            &CollectionMeta::new(
                &collection,
                Some("analytics text must remain analytics".to_string()),
            ),
        )
        .expect("collection");
    cassie
        .midge
        .save_constraints(
            &collection,
            &[FieldConstraint {
                default_value: Some(serde_json::json!("analytics")),
                ..FieldConstraint::new("value")
            }],
        )
        .expect("constraints");
    cassie
        .midge
        .put_index(&IndexMeta {
            collection: collection.clone(),
            name: "analytics".to_string(),
            field: "value".to_string(),
            fields: vec!["value".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        })
        .expect("index");
    cassie
        .midge
        .put_document(
            &collection,
            Some("row-1".to_string()),
            serde_json::json!({"value": "copy"}),
        )
        .expect("row");
}

async fn backup_database<R, W>(reader: &mut R, writer: &mut W) -> Vec<u8>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(&query_frame("BACKUP DATABASE analytics TO STDOUT"))
        .await
        .expect("backup query");
    writer.flush().await.expect("flush backup query");
    assert_eq!(support::read_wire_frame(reader).await.0, b'H');
    let mut image = Vec::new();
    loop {
        let frame = support::read_wire_frame(reader).await;
        match frame.0 {
            b'd' => image.extend_from_slice(&frame.1),
            b'c' => break,
            other => panic!("unexpected backup frame {other:?}"),
        }
    }
    assert_eq!(support::read_wire_frame(reader).await.0, b'C');
    assert_eq!(support::read_wire_frame(reader).await.0, b'Z');
    image
}

async fn restore_database<R, W>(reader: &mut R, writer: &mut W, image: &[u8])
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(&query_frame("RESTORE DATABASE restored FROM STDIN"))
        .await
        .expect("restore query");
    writer.flush().await.expect("flush restore query");
    assert_eq!(support::read_wire_frame(reader).await.0, b'G');
    for chunk in image.chunks(3) {
        writer
            .write_all(&copy_data_frame(chunk))
            .await
            .expect("restore data");
    }
    writer
        .write_all(&copy_done_frame())
        .await
        .expect("restore done");
    writer.flush().await.expect("flush restore data");
    assert_eq!(support::read_wire_frame(reader).await.0, b'C');
    assert_eq!(support::read_wire_frame(reader).await.0, b'Z');
}

fn assert_restored_database(cassie: &Cassie) {
    let collection = canonical_relation_name("restored", "public", "analytics");
    let restored = cassie
        .midge
        .get_document(&collection, "row-1")
        .expect("restored lookup")
        .expect("restored row");
    assert_eq!(restored.payload["value"], "copy");
    let metadata = cassie
        .midge
        .collection_metadata(&collection)
        .expect("restored collection metadata")
        .expect("restored collection");
    assert_eq!(
        metadata.description.as_deref(),
        Some("analytics text must remain analytics")
    );
    let constraints = cassie
        .midge
        .load_constraints(&collection)
        .expect("restored constraints");
    assert_eq!(
        constraints[0].default_value,
        Some(serde_json::json!("analytics"))
    );
    let indexes = cassie.catalog.list_indexes(&collection);
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].name, "analytics");
    assert_eq!(indexes[0].collection, collection);
}

#[test]
fn should_preserve_collection_name_matching_source_database_through_pgwire_restore() {
    // Arrange
    support::use_local_storage();
    let path = data_dir("round_trip");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        seed_database_image_fixture(&cassie);

        // Act
        let server = support::spawn_server(cassie.clone()).await;
        {
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            let image = backup_database(&mut reader, &mut write_half).await;
            restore_database(&mut reader, &mut write_half, &image).await;

            // Verify that a new session sees restored metadata without
            // requiring a process restart or manual catalog hydration.
            let mut restored_socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect restored database");
            let (restored_read, mut restored_write) = restored_socket.split();
            let mut restored_reader = tokio::io::BufReader::new(restored_read);
            let root_password = CassieRuntimeConfig::from_env()
                .expect("runtime config")
                .password;
            support::complete_startup_as(
                &mut restored_reader,
                &mut restored_write,
                "root",
                "restored",
                &root_password,
            )
            .await;
            restored_write
                .write_all(&query_frame("SELECT value FROM analytics"))
                .await
                .expect("query restored relation");
            restored_write.flush().await.expect("flush restored query");
            let restored_rows = support::read_frames_until_ready(&mut restored_reader).await;
            assert_eq!(support::error_code(&restored_rows), None);
            assert_eq!(
                support::data_rows(&restored_rows),
                vec![vec![Some("copy".to_string())]]
            );
            restored_write
                .shutdown()
                .await
                .expect("close restored pgwire client");
            write_half.shutdown().await.expect("close pgwire client");
        }

        // Assert
        assert_restored_database(&cassie);
        server.stop().await;
        tokio::task::yield_now().await;

        // Restart and assert persisted state.
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("restarted startup");

        // Assert
        assert_restored_database(&restarted);
        drop(restarted);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_qualified_restore_database_name_before_copy_in() {
    // Arrange
    support::use_local_storage();
    let path = data_dir("qualified-target");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    let (entered_copy_in, error_code) = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;

        // Act
        write_half
            .write_all(&query_frame("RESTORE DATABASE tenant.db FROM STDIN"))
            .await
            .expect("restore query");
        write_half.flush().await.expect("flush restore query");
        let first = support::read_wire_frame(&mut reader).await;
        let entered_copy_in = first.0 == b'G';
        let frames = if entered_copy_in {
            write_half
                .write_all(&support::copy_fail_frame("cancel invalid target probe"))
                .await
                .expect("cancel copy-in probe");
            write_half.flush().await.expect("flush copy fail");
            support::read_frames_until_ready(&mut reader).await
        } else {
            let mut frames = vec![first];
            frames.extend(support::read_frames_until_ready(&mut reader).await);
            frames
        };
        let error_code = support::error_code(&frames);
        server.stop().await;
        (entered_copy_in, error_code)
    });
    let _ = std::fs::remove_dir_all(path);

    // Assert
    assert!(
        !entered_copy_in,
        "invalid target must fail before CopyInResponse"
    );
    assert_eq!(error_code.as_deref(), Some("42601"));
}
