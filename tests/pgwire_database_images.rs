use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::types::{DataType, FieldSchema, Schema};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

#[path = "support/pgwire.rs"]
mod support;

fn data_dir(label: &str) -> String {
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

#[test]
fn should_stream_database_image_round_trip_through_pgwire_copy_messages() {
    // Arrange
    support::with_fallback();
    let path = data_dir("round_trip");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        cassie
            .midge
            .create_database("analytics", None)
            .expect("database");
        cassie
            .midge
            .create_collection(
                &canonical_relation_name("analytics", "public", "docs"),
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
                &canonical_relation_name("analytics", "public", "docs"),
                Some("row-1".to_string()),
                serde_json::json!({"value": "copy"}),
            )
            .expect("row");

        let server = support::spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;

        // Act: CopyOut backup.
        write_half
            .write_all(&query_frame("BACKUP DATABASE analytics TO STDOUT"))
            .await
            .expect("backup query");
        write_half.flush().await.expect("flush backup query");
        let copy_out = support::read_wire_frame(&mut reader).await;
        assert_eq!(copy_out.0, b'H');
        let mut image = Vec::new();
        loop {
            let frame = support::read_wire_frame(&mut reader).await;
            match frame.0 {
                b'd' => image.extend_from_slice(&frame.1),
                b'c' => break,
                other => panic!("unexpected backup frame {other:?}"),
            }
        }
        let command = support::read_wire_frame(&mut reader).await;
        assert_eq!(command.0, b'C');
        let ready = support::read_wire_frame(&mut reader).await;
        assert_eq!(ready.0, b'Z');

        // Act: CopyIn restore, deliberately fragmented into small CopyData messages.
        write_half
            .write_all(&query_frame("RESTORE DATABASE restored FROM STDIN"))
            .await
            .expect("restore query");
        write_half.flush().await.expect("flush restore query");
        let copy_in = support::read_wire_frame(&mut reader).await;
        assert_eq!(copy_in.0, b'G');
        for chunk in image.chunks(3) {
            write_half
                .write_all(&copy_data_frame(chunk))
                .await
                .expect("restore data");
        }
        write_half
            .write_all(&copy_done_frame())
            .await
            .expect("restore done");
        write_half.flush().await.expect("flush restore data");
        let command = support::read_wire_frame(&mut reader).await;
        assert_eq!(command.0, b'C');
        let ready = support::read_wire_frame(&mut reader).await;
        assert_eq!(ready.0, b'Z');

        // Assert
        let restored = cassie
            .midge
            .get_document(
                &canonical_relation_name("restored", "public", "docs"),
                "row-1",
            )
            .expect("restored lookup")
            .expect("restored row");
        assert_eq!(restored.payload["value"], "copy");

        write_half.shutdown().await.expect("close pgwire client");
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}
