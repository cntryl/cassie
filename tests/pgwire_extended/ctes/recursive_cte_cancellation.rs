//! Recursive CTE cancellation through PostgreSQL wire protocol, tracked by #801.

use std::time::Duration;

use crate::support_pgwire as wire;
use cassie::app::Cassie;
use tokio::io::AsyncWriteExt;

struct FixtureDirectory(String);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
fn should_cancel_recursive_cte_execution_without_publishing_partial_rows() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-801-recursive-cte-cancel");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();

    // Act
    let (baseline, cancelled, recovery) = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie
            .startup()
            .expect("start recursive CTE cancellation fixture");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("recursive CTE query connection");
        let (mut reader, mut writer) = socket.into_split();
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "WITH RECURSIVE walk(n) AS ( \
                    VALUES (1::INTEGER) \
                    UNION ALL \
                    SELECT n + 1 FROM walk \
                    WHERE n < 3 \
                      AND pg_sleep_for(INTERVAL '0 seconds') IS NOT NULL \
                 ) SELECT COUNT(*) FROM walk",
            )],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "WITH RECURSIVE walk(n) AS ( \
                    VALUES (1::INTEGER) \
                    UNION ALL \
                    SELECT n + 1 FROM walk \
                    WHERE n < 3 \
                      AND pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL \
                 ) SELECT COUNT(*) FROM walk",
            )],
        )
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
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
        let cancelled =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 45")]).await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.stop().await;
        (baseline, cancelled, recovery)
    });
    drop(runtime);

    // Assert
    assert_eq!(wire::error_code(&baseline), None);
    let baseline_description = baseline
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("recursive CTE baseline RowDescription");
    assert_eq!(
        wire::parse_row_description(&baseline_description.1)[0].type_oid,
        20
    );
    assert_eq!(wire::data_rows(&baseline), vec![vec![Some("3".to_owned())]]);

    assert_eq!(wire::error_code(&cancelled).as_deref(), Some("57014"));
    assert_eq!(
        wire::data_rows(&cancelled),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(cancelled.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(cancelled.last().map(|frame| frame.0), Some(b'Z'));

    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("45".to_owned())]]
    );
}
