//! Real wire cancellation synchronized at the selected SQL sleep boundary.

use std::time::Duration;

use crate::support_pgwire as wire;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Frames = Vec<(u8, Vec<u8>)>;

pub fn run_sleep_cancellation(sql: &str) -> (Frames, Frames) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
        cassie.startup().expect("start sleep cancellation fixture");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (mut reader, mut writer) = socket.into_split();
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
        let (_pause_guard, selected, resume) = cassie::runtime::pause_next_query_at_sleep_entry();
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
        tokio::task::spawn_blocking(move || selected.recv_timeout(Duration::from_secs(5)))
            .await
            .expect("sleep-entry observer task")
            .expect("query must enter the sleep function before cancellation");

        let mut cancel_socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write cancel request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancel request");
        let mut response = [0; 1];
        let received =
            tokio::time::timeout(Duration::from_secs(5), cancel_socket.read(&mut response))
                .await
                .expect("CancelRequest handler must finish")
                .expect("wait for CancelRequest processing");
        assert_eq!(received, 0, "CancelRequest has no response body");
        resume
            .send(())
            .expect("resume after cancellation was handled");
        let canceled =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 45")]).await;
        let recovered =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        drop(reader);
        drop(writer);
        server.stop().await;
        (canceled, recovered)
    })
}

pub fn assert_canceled_and_recovered(canceled: &Frames, recovered: &Frames) {
    assert_eq!(wire::error_code(canceled).as_deref(), Some("57014"));
    assert!(!canceled.iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')));
    assert_eq!(canceled.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(wire::error_code(recovered), None);
    assert_eq!(
        wire::data_rows(recovered),
        vec![vec![Some("45".to_owned())]]
    );
}
