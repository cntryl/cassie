use super::*;

type WireFrame = (u8, Vec<u8>);

async fn read_until_first_data_row(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<WireFrame>,
    frames: &mut Vec<WireFrame>,
) -> bool {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Some(frame) = receiver.recv().await else {
                return false;
            };
            let tag = frame.0;
            frames.push(frame);
            if tag == b'D' {
                return true;
            }
            if tag == b'E' || tag == b'Z' {
                return false;
            }
        }
    })
    .await
    .unwrap_or(false)
}

async fn read_until_ready(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<WireFrame>,
    frames: &mut Vec<WireFrame>,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Some(frame) = receiver.recv().await else {
                return;
            };
            let ready = frame.0 == b'Z';
            frames.push(frame);
            if ready {
                return;
            }
        }
    })
    .await
    .expect("the pgwire command should finish with ReadyForQuery");
}

async fn send_cancel(address: std::net::SocketAddr, process_id: i32, secret_key: i32) {
    let mut cancel_socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect the CancelRequest socket");
    cancel_socket
        .write_all(&wire::cancel_request_frame(process_id, secret_key))
        .await
        .expect("send CancelRequest after observing the streamed row");
    cancel_socket
        .shutdown()
        .await
        .expect("close the CancelRequest socket");
}

#[test]
fn should_preserve_json_recordset_rows_emitted_before_cancellation() {
    // Arrange
    let (storage, config) = configured_fixture("json-recordset-midstream-cancel", 100);
    let runtime = current_thread_runtime();
    let first_payload = "x".repeat(512);
    let document = serde_json::json!([
        {"n": 1, "payload": first_payload.clone()},
        {"n": 2, "payload": "blocked"},
        {"n": 3, "payload": "later"}
    ])
    .to_string()
    .replace('\'', "''");
    let query = format!(
        "SELECT item.n, item.payload \
         FROM jsonb_to_recordset('{document}'::JSONB) \
           AS item(n INTEGER, payload TEXT) \
         WHERE CASE WHEN item.n = 2 \
           THEN pg_sleep_for(INTERVAL '60 seconds') IS NULL \
           ELSE TRUE END"
    );

    // Act
    let (first_data_row_arrived, cancelled, recovery) = runtime.block_on(async {
        let (address, server) = start_server(storage.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("query connection");
        let (mut reader, mut writer) = tokio::io::split(socket);
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
        let (frame_sender, mut frame_receiver) = tokio::sync::mpsc::unbounded_channel();
        let frame_reader = tokio::spawn(async move {
            loop {
                let frame = wire::read_wire_frame(&mut reader).await;
                if frame_sender.send(frame).is_err() {
                    return;
                }
            }
        });

        wire::write_frames(&mut writer, vec![wire::simple_query_frame(&query)]).await;
        let mut cancelled = Vec::new();
        let first_data_row_arrived =
            read_until_first_data_row(&mut frame_receiver, &mut cancelled).await;

        if first_data_row_arrived {
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        while let Ok(frame) = frame_receiver.try_recv() {
            cancelled.push(frame);
        }
        let query_already_ended = cancelled
            .iter()
            .any(|(tag, _)| *tag == b'E' || *tag == b'Z');
        if !query_already_ended {
            send_cancel(address, process_id, secret_key).await;
        }
        if !cancelled.iter().any(|(tag, _)| *tag == b'Z') {
            read_until_ready(&mut frame_receiver, &mut cancelled).await;
        }

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 45")]).await;
        let mut recovery = Vec::new();
        read_until_ready(&mut frame_receiver, &mut recovery).await;

        drop(writer);
        frame_reader.abort();
        let _ = frame_reader.await;
        server.abort();
        let _ = server.await;
        (first_data_row_arrived, cancelled, recovery)
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert!(
        first_data_row_arrived,
        "cancellation should be triggered only after a complete result DataRow arrived; \
         SQLSTATE: {:?}",
        wire::error_code(&cancelled)
    );
    assert_eq!(wire::error_code(&cancelled).as_deref(), Some("57014"));
    assert_eq!(
        wire::data_rows(&cancelled),
        vec![vec![Some("1".to_owned()), Some(first_payload)]]
    );
    assert_eq!(cancelled.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    let ready = cancelled
        .iter()
        .find(|(tag, _)| *tag == b'Z')
        .expect("cancelled statement should return ReadyForQuery");
    assert_eq!(ready.1, b"I");

    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("45".to_owned())]]
    );
    let recovery_ready = recovery
        .iter()
        .find(|(tag, _)| *tag == b'Z')
        .expect("recovery query should return ReadyForQuery");
    assert_eq!(recovery_ready.1, b"I");
}
